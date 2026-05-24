//! Thread-local UI perf collector.
//!
//! Lets us answer the question "where is the frame time going?" without
//! external profilers. The collector lives in a `thread_local!` so render
//! code can drop `PerfScope::enter("name")` anywhere on the foreground
//! thread without plumbing context. It aggregates by scope name over a
//! 1-second window and dumps once per second to stderr when enabled.
//!
//! Enable with `FUTUREBOARD_UI_PERF=1` (or `=verbose` to also dump every
//! repaint instead of every second).
//!
//! Cost when disabled: one thread-local flag check per scope/count call.
//! When enabled: one `Instant::now()` + a small HashMap insert per scope.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

#[derive(Default, Clone, Copy)]
struct ScopeAgg {
    total_ns: u64,
    count: u64,
    max_ns: u64,
}

#[derive(Default)]
struct CounterAgg {
    last: u64,
    max: u64,
}

/// Names of scopes that wrap a single child of `StudioLayout`. Used to
/// compute a "self time" for the root scope by subtracting child sums,
/// so the verdict line attributes time to the right component instead
/// of always blaming `StudioLayout`.
const ROOT_SCOPE: &str = "StudioLayout";
const ROOT_CHILDREN: &[&str] = &[
    "AppChrome",
    "Sidebar",
    "Timeline",
    "Inspector",
    "BottomPanel",
    "StatusBar",
];

struct Collector {
    enabled: bool,
    /// Aggregated time per named scope this window.
    scopes: BTreeMap<&'static str, ScopeAgg>,
    /// Latest-value counters (e.g. visible_browser_rows, grid_lines).
    /// We store the most recent sample plus max-this-window so the log
    /// is meaningful even when the value bounces.
    counters: BTreeMap<&'static str, CounterAgg>,
    /// Per-second frame stats.
    frame_count: u64,
    frame_total_ms: f32,
    frame_min_ms: f32,
    frame_max_ms: f32,
    last_frame: Option<Instant>,
    window_start: Instant,
    last_repaint_reason: &'static str,
    /// Per-reason frame count this window — drives the repaint-source
    /// attribution and the idle-loop detector.
    reason_counts: BTreeMap<&'static str, u64>,
}

impl Collector {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            enabled: std::env::var_os("FUTUREBOARD_UI_PERF").is_some(),
            scopes: BTreeMap::new(),
            counters: BTreeMap::new(),
            frame_count: 0,
            frame_total_ms: 0.0,
            frame_min_ms: f32::INFINITY,
            frame_max_ms: 0.0,
            last_frame: None,
            window_start: now,
            last_repaint_reason: "init",
            reason_counts: BTreeMap::new(),
        }
    }

    fn add_scope(&mut self, name: &'static str, ns: u64) {
        let entry = self.scopes.entry(name).or_default();
        entry.total_ns = entry.total_ns.saturating_add(ns);
        entry.count = entry.count.saturating_add(1);
        if ns > entry.max_ns {
            entry.max_ns = ns;
        }
    }

    fn record_counter(&mut self, name: &'static str, value: u64) {
        let entry = self.counters.entry(name).or_default();
        entry.last = value;
        if value > entry.max {
            entry.max = value;
        }
    }

    fn tick_frame(&mut self, reason: &'static str) -> bool {
        let now = Instant::now();
        if let Some(prev) = self.last_frame {
            let dt_ms = now.duration_since(prev).as_secs_f32() * 1000.0;
            if dt_ms < 1000.0 {
                self.frame_total_ms += dt_ms;
                self.frame_count += 1;
                if dt_ms > self.frame_max_ms {
                    self.frame_max_ms = dt_ms;
                }
                if dt_ms < self.frame_min_ms {
                    self.frame_min_ms = dt_ms;
                }
            }
        }
        self.last_frame = Some(now);
        self.last_repaint_reason = reason;
        *self.reason_counts.entry(reason).or_insert(0) += 1;
        now.duration_since(self.window_start) >= Duration::from_secs(1)
    }

    fn flush(&mut self) {
        if !self.enabled {
            self.reset_window();
            return;
        }
        let elapsed = self.window_start.elapsed().as_secs_f32().max(0.001);
        let fps = self.frame_count as f32 / elapsed;
        let avg_ms = if self.frame_count > 0 {
            self.frame_total_ms / self.frame_count as f32
        } else {
            0.0
        };
        let min_ms = if self.frame_min_ms.is_finite() {
            self.frame_min_ms
        } else {
            0.0
        };

        // ── Repaint-source attribution ─────────────────────────────
        // Sort reasons by frame count, descending. The top reason is
        // what's driving the repaint cadence this second.
        let mut reasons: Vec<(&&'static str, &u64)> = self.reason_counts.iter().collect();
        reasons.sort_by(|a, b| b.1.cmp(a.1));
        let dominant_reason = reasons
            .first()
            .map(|(name, count)| (**name, **count))
            .unwrap_or(("none", 0));
        let reason_pct = if self.frame_count > 0 {
            100.0 * dominant_reason.1 as f32 / self.frame_count as f32
        } else {
            0.0
        };

        eprintln!(
            "[ui-perf] fps={:.1} avg={:.2}ms min={:.2}ms max={:.2}ms repaint={}/s dominant_reason={}({:.0}%)",
            fps, avg_ms, min_ms, self.frame_max_ms, self.frame_count, dominant_reason.0, reason_pct
        );

        if !reasons.is_empty() && reasons.len() > 1 {
            let mut line = String::from("[ui-perf] repaint-sources: ");
            for (name, count) in &reasons {
                line.push_str(&format!("{}={}  ", name, count));
            }
            eprintln!("{}", line.trim_end());
        }

        // ── Scope ranking ───────────────────────────────────────────
        // Subtract instrumented child totals from the root to derive
        // "root self time" — otherwise StudioLayout always wins because
        // it transitively includes every child.
        let mut ranked: Vec<(&'static str, ScopeAgg)> = self
            .scopes
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect();
        let children_sum_ns: u64 = ranked
            .iter()
            .filter(|(n, _)| ROOT_CHILDREN.contains(n))
            .map(|(_, a)| a.total_ns)
            .sum();
        if let Some((_, root)) = ranked.iter_mut().find(|(n, _)| *n == ROOT_SCOPE) {
            let self_ns = root.total_ns.saturating_sub(children_sum_ns);
            root.total_ns = self_ns;
            root.max_ns = 0;
        }
        ranked.retain(|(_, a)| a.count > 0 && a.total_ns > 0);
        ranked.sort_by(|a, b| b.1.total_ns.cmp(&a.1.total_ns));

        let total_ns: u64 = ranked.iter().map(|(_, a)| a.total_ns).sum();
        let window_ns = (elapsed * 1_000_000_000.0) as u64;

        if !ranked.is_empty() {
            let mut line = String::from("[ui-perf] ranked: ");
            for (name, agg) in ranked.iter().take(8) {
                let total_ms = agg.total_ns as f32 / 1_000_000.0;
                let pct = if total_ns > 0 {
                    100.0 * agg.total_ns as f32 / total_ns as f32
                } else {
                    0.0
                };
                let display_name = if *name == ROOT_SCOPE {
                    "StudioLayout(self)"
                } else {
                    *name
                };
                line.push_str(&format!(
                    "{}={:.2}ms({:.0}%,x{})  ",
                    display_name, total_ms, pct, agg.count
                ));
            }
            eprintln!("{}", line.trim_end());
        }

        // ── Verdict ─────────────────────────────────────────────────
        // The whole point: tell the next pass exactly where to cut,
        // and whether an idle-repaint loop is active.
        let verdict = self.compute_verdict(&ranked, &dominant_reason, fps, window_ns);
        eprintln!("[ui-perf] VERDICT: {}", verdict);

        // Mirror the latest verdict to a small file so the next agent
        // pass (or the user) can read it without re-running. Path is
        // overridable via FUTUREBOARD_UI_PERF_LOG; defaults to
        // `target/ui-perf-last.txt` next to the build artifacts.
        let log_path = std::env::var("FUTUREBOARD_UI_PERF_LOG")
            .unwrap_or_else(|_| "target/ui-perf-last.txt".to_string());
        let payload = format!(
            "fps={:.1}\navg_ms={:.2}\nmax_ms={:.2}\nrepaint_per_s={}\ndominant_reason={} ({}/{} frames)\nverdict={}\n\nranked:\n{}\n\ncounts:\n{}\n",
            fps,
            avg_ms,
            self.frame_max_ms,
            self.frame_count,
            dominant_reason.0,
            dominant_reason.1,
            self.frame_count,
            verdict,
            ranked
                .iter()
                .take(8)
                .map(|(n, a)| format!(
                    "  {} = {:.2}ms/s (x{})",
                    if *n == ROOT_SCOPE { "StudioLayout(self)" } else { *n },
                    a.total_ns as f32 / 1_000_000.0,
                    a.count
                ))
                .collect::<Vec<_>>()
                .join("\n"),
            self.counters
                .iter()
                .map(|(n, c)| format!("  {} = {} (max {})", n, c.last, c.max))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        // Best-effort write — we don't want a missing target/ dir to
        // crash the render thread. log_err equivalent: ignore.
        let _ = std::fs::write(&log_path, payload);

        if !self.counters.is_empty() {
            let mut line = String::from("[ui-perf] counts: ");
            for (name, agg) in &self.counters {
                if agg.last == agg.max {
                    line.push_str(&format!("{}={}  ", name, agg.last));
                } else {
                    line.push_str(&format!("{}={}(max {})  ", name, agg.last, agg.max));
                }
            }
            eprintln!("{}", line.trim_end());
        }

        self.reset_window();
    }

    /// Derive a one-line, action-oriented verdict the next optimization
    /// pass can act on without further measurement. Two axes:
    ///   1. Is there a repaint-rate problem? (idle loop, runaway notify)
    ///   2. Where is the per-frame time going? (top scope by total ms/s)
    fn compute_verdict(
        &self,
        ranked: &[(&'static str, ScopeAgg)],
        dominant_reason: &(&'static str, u64),
        fps: f32,
        window_ns: u64,
    ) -> String {
        // Idle loop: predominantly idle/interaction reason but still
        // hitting > ~12 fps means something is dirty-marking the view
        // even though the user isn't doing anything meaningful.
        let mut parts: Vec<String> = Vec::new();
        let idle_loop = dominant_reason.0 == "idle/interaction"
            && dominant_reason.1 >= 50 * self.frame_count.max(1) / 100
            && fps > 12.0;
        if idle_loop {
            parts.push(format!(
                "IDLE-REPAINT-LOOP ({} idle frames/s, {:.0} fps) — \
                 something is calling cx.notify without user input. \
                 Likely culprits: spawn_audio_poll cadence, \
                 smooth_scroll_towards_target convergence, or a \
                 meter/scroll animation that never terminates.",
                dominant_reason.1, fps
            ));
        }

        if let Some((top_name, top_agg)) = ranked.first() {
            let top_ms_s = top_agg.total_ns as f32 / 1_000_000.0;
            let budget_pct = if window_ns > 0 {
                100.0 * top_agg.total_ns as f32 / window_ns as f32
            } else {
                0.0
            };
            let display = if *top_name == ROOT_SCOPE {
                "StudioLayout(self)"
            } else {
                top_name
            };
            // If a single scope eats > 25% of the 1-second window,
            // it's the obvious cut target.
            if budget_pct > 25.0 {
                parts.push(format!(
                    "TOP-SCOPE {} = {:.1}ms/s ({:.0}% of window) — cut this first.",
                    display, top_ms_s, budget_pct
                ));
            } else {
                parts.push(format!(
                    "TOP-SCOPE {} = {:.1}ms/s ({:.0}% of window).",
                    display, top_ms_s, budget_pct
                ));
            }
        }

        // Frame-time pass/fail vs the 60 Hz budget. We use the average,
        // not the max, so a single hitched frame doesn't dominate.
        let avg_ms = if self.frame_count > 0 {
            self.frame_total_ms / self.frame_count as f32
        } else {
            0.0
        };
        if avg_ms > 16.67 && self.frame_count > 0 {
            parts.push(format!(
                "FRAME-BUDGET FAIL — avg {:.2}ms exceeds 16.67ms (60Hz target).",
                avg_ms
            ));
        } else if self.frame_count > 0 {
            parts.push(format!(
                "FRAME-BUDGET OK — avg {:.2}ms within 16.67ms.",
                avg_ms
            ));
        }

        if parts.is_empty() {
            "no data".to_string()
        } else {
            parts.join(" | ")
        }
    }

    fn reset_window(&mut self) {
        self.scopes.clear();
        self.reason_counts.clear();
        for c in self.counters.values_mut() {
            c.max = c.last;
        }
        self.frame_count = 0;
        self.frame_total_ms = 0.0;
        self.frame_min_ms = f32::INFINITY;
        self.frame_max_ms = 0.0;
        self.window_start = Instant::now();
    }
}

thread_local! {
    static COLLECTOR: RefCell<Collector> = RefCell::new(Collector::new());
}

/// Returns whether perf instrumentation is active. Cheap — single TLS
/// read. Callers can use this to skip building counter labels when
/// disabled, though `count` is already a no-op when disabled.
pub fn enabled() -> bool {
    COLLECTOR.with(|c| c.borrow().enabled)
}

/// Record a named counter (visible row count, grid line count, etc.).
/// Cheap no-op when disabled. Last write wins for the log line; the
/// per-window max is also retained.
pub fn count(name: &'static str, value: u64) {
    COLLECTOR.with(|c| {
        let mut c = c.borrow_mut();
        if !c.enabled {
            return;
        }
        c.record_counter(name, value);
    });
}

/// RAII timing scope. Records elapsed wall time into the named bucket
/// on drop. No-op when perf is disabled.
pub struct PerfScope {
    name: &'static str,
    start: Option<Instant>,
}

impl PerfScope {
    pub fn enter(name: &'static str) -> Self {
        let start = COLLECTOR.with(|c| c.borrow().enabled.then(Instant::now));
        Self { name, start }
    }
}

impl Drop for PerfScope {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            let ns = start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
            let name = self.name;
            COLLECTOR.with(|c| c.borrow_mut().add_scope(name, ns));
        }
    }
}

/// Called from the root render once per frame. `reason` is a short
/// label (e.g. "transport", "menu", "idle/interaction"). Returns the
/// formatted HUD string and flushes the per-second log when the
/// window rolls over.
pub fn tick_root_frame(reason: &'static str) {
    let should_flush = COLLECTOR.with(|c| c.borrow_mut().tick_frame(reason));
    if should_flush {
        COLLECTOR.with(|c| c.borrow_mut().flush());
    }
}
