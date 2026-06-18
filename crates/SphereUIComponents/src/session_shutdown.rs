//! Awaitable session-runtime shutdown — unloads plugins, shuts down bridge
//! hosts, and reports progress for Loading Session / project-switch UI.

use std::time::{Duration, Instant};

use crate::components::progress_dialog::ProgressBarValue;
use crate::layout::plugin_bridge_runtime::SharedPluginBridgeRuntime;

const PLUGIN_UNLOAD_WAIT: Duration = Duration::from_millis(400);
const HOST_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionShutdownReason {
    ProjectClose,
    ProjectSwitch,
    ProjectReplace,
    AppExit,
}

impl SessionShutdownReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::ProjectClose => "project_close",
            Self::ProjectSwitch => "project_switch",
            Self::ProjectReplace => "project_replace",
            Self::AppExit => "app_exit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionTransitionPhase {
    ClosingSession,
    LoadingSession,
    SwitchingSession,
}

#[derive(Debug, Clone)]
pub struct SessionTransitionProgress {
    pub phase: SessionTransitionPhase,
    pub title: String,
    pub stage: String,
    pub detail: Option<String>,
    pub current: Option<usize>,
    pub total: Option<usize>,
    pub bar: ProgressBarValue,
}

impl SessionTransitionProgress {
    pub fn indeterminate(
        phase: SessionTransitionPhase,
        title: impl Into<String>,
        stage: impl Into<String>,
    ) -> Self {
        Self {
            phase,
            title: title.into(),
            stage: stage.into(),
            detail: None,
            current: None,
            total: None,
            bar: ProgressBarValue::Indeterminate,
        }
    }

    pub fn indexed(
        phase: SessionTransitionPhase,
        title: impl Into<String>,
        stage: impl Into<String>,
        current: usize,
        total: usize,
        base: f32,
        span: f32,
    ) -> Self {
        let total = total.max(1);
        let p = base + span * (current as f32 / total as f32);
        Self {
            phase,
            title: title.into(),
            stage: stage.into(),
            detail: None,
            current: Some(current),
            total: Some(total),
            bar: ProgressBarValue::value(p.clamp(0.0, 1.0)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PluginUnloadTarget {
    pub track_id: String,
    pub insert_id: String,
    pub display_name: String,
    pub track_name: String,
    pub is_instrument: bool,
}

#[derive(Clone)]
pub struct SessionShutdownSnapshot {
    pub reason: SessionShutdownReason,
    pub plugin_targets: Vec<PluginUnloadTarget>,
    pub bridge_runtime: Option<SharedPluginBridgeRuntime>,
    pub instrument_track_ids: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SessionShutdownReport {
    pub plugins_unloaded: usize,
    pub plugins_failed: usize,
    pub hosts_shutdown: usize,
    pub hosts_killed: usize,
    pub warnings: Vec<String>,
}

pub fn run_session_shutdown(
    mut snapshot: SessionShutdownSnapshot,
    mut progress: impl FnMut(SessionTransitionProgress),
) -> SessionShutdownReport {
    let phase = match snapshot.reason {
        SessionShutdownReason::ProjectSwitch | SessionShutdownReason::ProjectReplace => {
            SessionTransitionPhase::SwitchingSession
        }
        _ => SessionTransitionPhase::ClosingSession,
    };
    let title = match snapshot.reason {
        SessionShutdownReason::ProjectSwitch | SessionShutdownReason::ProjectReplace => {
            "Switching Project…"
        }
        _ => "Closing Session…",
    };

    eprintln!("[SessionShutdown] begin reason={}", snapshot.reason.label());

    let mut report = SessionShutdownReport::default();

    progress(SessionTransitionProgress::indeterminate(
        phase,
        title,
        "Stopping transport",
    ));
    eprintln!("[SessionShutdown] stop transport");

    progress(SessionTransitionProgress::indeterminate(
        phase,
        title,
        "Releasing notes",
    ));
    eprintln!("[SessionShutdown] all-notes-off");

    progress(SessionTransitionProgress::indeterminate(
        phase,
        title,
        "Closing plugin editors",
    ));

    let plugin_total = snapshot.plugin_targets.len().max(1);
    for (index, target) in snapshot.plugin_targets.iter().enumerate() {
        let current = index + 1;
        progress(SessionTransitionProgress::indexed(
            phase,
            title,
            format!(
                "Unloading plugins {}/{}: {} on {}",
                current,
                snapshot.plugin_targets.len().max(1),
                target.display_name,
                target.track_name
            ),
            current,
            plugin_total,
            0.1,
            0.55,
        ));
        eprintln!(
            "[PluginUnload] unloading plugin {}/{} name={} instance_id={}",
            current,
            snapshot.plugin_targets.len(),
            target.display_name,
            target.insert_id
        );

        if let Some(runtime) = snapshot.bridge_runtime.as_ref() {
            match unload_bridge_plugin(runtime, &target.insert_id) {
                Ok(()) => {
                    report.plugins_unloaded += 1;
                    eprintln!("[PluginUnload] unloaded instance_id={}", target.insert_id);
                }
                Err(warning) => {
                    report.plugins_failed += 1;
                    report.warnings.push(warning);
                }
            }
        } else {
            report.plugins_unloaded += 1;
        }
    }

    if let Some(runtime) = snapshot.bridge_runtime.take() {
        let host_pid = runtime.lock().ok().and_then(|bridge| bridge.host_pid());
        progress(SessionTransitionProgress::indeterminate(
            phase,
            title,
            if let Some(pid) = host_pid {
                format!("Shutting down plugin host pid={pid}")
            } else {
                "Shutting down plugin hosts".to_string()
            },
        ));
        let host_report = crate::layout::plugin_bridge_runtime::shutdown_bridge_runtime(
            Some(runtime),
            HOST_SHUTDOWN_TIMEOUT,
            |stage, bar| {
                progress(SessionTransitionProgress {
                    phase,
                    title: title.to_string(),
                    stage,
                    detail: None,
                    current: None,
                    total: None,
                    bar,
                });
            },
        );
        report.hosts_shutdown += host_report.hosts_shutdown;
        report.hosts_killed += host_report.hosts_killed;
        report.warnings.extend(host_report.warnings);
    }

    let remaining =
        SpherePluginHost::plugin_host_lifecycle::BridgeHostManager::global().host_count();
    eprintln!("[SessionShutdown] complete remaining_hosts={remaining}");

    progress(SessionTransitionProgress::indeterminate(
        phase,
        title,
        "Session closed",
    ));

    if remaining > 0 {
        report.warnings.push(format!(
            "{remaining} plugin host process(es) still tracked after shutdown"
        ));
    }

    report
}

fn unload_bridge_plugin(
    runtime: &SharedPluginBridgeRuntime,
    insert_id: &str,
) -> Result<(), String> {
    let Ok(mut bridge) = runtime.lock() else {
        return Err(format!("bridge lock poisoned for instance={insert_id}"));
    };
    if !bridge.is_loaded(insert_id) {
        return Ok(());
    }
    bridge.unload_plugin(insert_id.to_string());
    let deadline = Instant::now() + PLUGIN_UNLOAD_WAIT;
    while Instant::now() < deadline {
        bridge.drain_events();
        if !bridge.is_loaded(insert_id) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    if bridge.is_loaded(insert_id) {
        return Err(format!("plugin unload timed out instance_id={insert_id}"));
    }
    Ok(())
}
