//! Filter counts and multi-filter matching.

use std::collections::BTreeMap;

use SpherePluginHost::{PluginFormat, PluginKind, PluginScanStatus, RegistryPlugin};

use crate::components::plugin_picker::category::normalized_category_label;
use crate::components::plugin_picker::prefs::PluginPickerPrefs;
use crate::components::plugin_picker::search_index::PluginSearchIndex;
use crate::components::plugin_picker::state::{PickerFilter, PluginFilterState};

#[derive(Debug, Clone, Default)]
pub struct FilterCounts {
    pub all: usize,
    pub favorites: usize,
    pub recent: usize,
    pub instruments: usize,
    pub effects: usize,
    pub vst3: usize,
    pub clap: usize,
    pub au: usize,
    pub failed: usize,
}

#[derive(Debug, Clone)]
pub struct FilterResult {
    pub indices: Vec<usize>,
    pub counts: FilterCounts,
    pub vendors: Vec<String>,
    pub categories: Vec<String>,
}

/// Cached `FUTUREBOARD_PLUGIN_PICKER_DEBUG` flag — gates the picker perf timing
/// lines used to localize typing-latency. Read once, not per keypress.
pub fn picker_perf_debug() -> bool {
    use std::sync::OnceLock;
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_PLUGIN_PICKER_DEBUG").is_some())
}

pub fn compute_filter_result(
    index: &PluginSearchIndex,
    query: &str,
    filters: &PluginFilterState,
    prefs: &PluginPickerPrefs,
    debug_mode: bool,
) -> FilterResult {
    let perf = picker_perf_debug();
    let started = perf.then(std::time::Instant::now);
    let plugins = index.plugins();
    let q = query.trim().to_ascii_lowercase();
    let vendor_filter = filters
        .vendor
        .as_ref()
        .map(|vendor| vendor.to_ascii_lowercase());
    let category_filter = filters
        .category
        .as_ref()
        .map(|category| category.to_ascii_lowercase());

    let mut counts = FilterCounts::default();
    let mut indices = Vec::new();

    // Single allocation-free pass: tally library-wide counts and collect the
    // matching row indices. The sidebar vendor/category facets are NOT rebuilt
    // here — they are precomputed once on the shared index — so typing only pays
    // for the cheap substring/enum checks below, never per-keypress String work.
    for (idx, plugin) in plugins.iter().enumerate() {
        update_counts(&mut counts, plugin, prefs, debug_mode);

        if !matches_sidebar(&filters.sidebar, plugin, prefs, debug_mode) {
            continue;
        }
        if let Some(fmt) = filters.format {
            if plugin.format != fmt {
                continue;
            }
        }
        if let Some(vendor) = vendor_filter.as_deref() {
            if index.vendor_lower(idx) != vendor {
                continue;
            }
        }
        if let Some(category) = category_filter.as_deref() {
            if index.category_lower(idx) != category {
                continue;
            }
        }
        if !q.is_empty() && !index.search_text(idx).contains(&q) {
            continue;
        }
        indices.push(idx);
    }

    if let Some(started) = started {
        eprintln!(
            "[picker-perf] compute_filter_result q={:?} plugins={} results={} took_us={}",
            q,
            plugins.len(),
            indices.len(),
            started.elapsed().as_micros()
        );
    }

    FilterResult {
        indices,
        counts,
        vendors: index.sidebar_vendors().to_vec(),
        categories: index.sidebar_categories().to_vec(),
    }
}

fn update_counts(
    counts: &mut FilterCounts,
    plugin: &RegistryPlugin,
    prefs: &PluginPickerPrefs,
    debug_mode: bool,
) {
    counts.all += 1;
    if prefs.is_favorite(&plugin.id) {
        counts.favorites += 1;
    }
    if prefs.recent.contains(&plugin.id) {
        counts.recent += 1;
    }
    if plugin.kind == PluginKind::Instrument {
        counts.instruments += 1;
    } else {
        counts.effects += 1;
    }
    match plugin.format {
        PluginFormat::Vst3 => counts.vst3 += 1,
        PluginFormat::Clap => counts.clap += 1,
        PluginFormat::Au => counts.au += 1,
        _ => {}
    }
    if debug_mode && is_failed_plugin(plugin) {
        counts.failed += 1;
    }
}

fn is_failed_plugin(plugin: &RegistryPlugin) -> bool {
    !plugin.scan_status.is_usable()
        || matches!(
            plugin.scan_status,
            PluginScanStatus::Failed | PluginScanStatus::Crashed | PluginScanStatus::MetadataOnly
        )
}

fn matches_sidebar(
    filter: &PickerFilter,
    plugin: &RegistryPlugin,
    prefs: &PluginPickerPrefs,
    debug_mode: bool,
) -> bool {
    match filter {
        PickerFilter::All => true,
        PickerFilter::Favorites => prefs.is_favorite(&plugin.id),
        PickerFilter::RecentlyUsed => prefs.recent.contains(&plugin.id),
        PickerFilter::Instruments => plugin.kind == PluginKind::Instrument,
        PickerFilter::Effects => plugin.kind == PluginKind::Effect,
        PickerFilter::Format(fmt) => plugin.format == *fmt,
        PickerFilter::Vendor(v) => plugin.vendor.eq_ignore_ascii_case(v),
        PickerFilter::Category(c) => normalized_category_label(plugin).eq_ignore_ascii_case(c),
        PickerFilter::Failed => debug_mode && is_failed_plugin(plugin),
    }
}

pub fn vendor_counts(plugins: &[RegistryPlugin]) -> BTreeMap<String, usize> {
    let mut map = BTreeMap::new();
    for plugin in plugins {
        if plugin.vendor.is_empty() {
            continue;
        }
        *map.entry(plugin.vendor.clone()).or_insert(0) += 1;
    }
    map
}

#[cfg(test)]
mod perf_tests {
    use super::*;
    use crate::components::plugin_picker::search_index::PluginSearchIndex;
    use crate::components::plugin_picker::state::PickerFilter;
    use std::path::PathBuf;
    use std::time::Instant;
    use SpherePluginHost::PluginStatus;

    fn synth_plugins(n: usize) -> Vec<RegistryPlugin> {
        let vendors = [
            "FabFilter",
            "Acme",
            "Antares",
            "IK Multimedia",
            "Ample Sound",
            "Celemony",
            "u-he",
            "Valhalla",
            "Native Instruments",
            "Waves",
        ];
        let cats = [
            "EQ", "Dynamics", "Reverb", "Delay", "Analyzer", "Synth", "Other",
        ];
        (0..n)
            .map(|i| RegistryPlugin {
                id: format!("vendor.plugin.{i}"),
                name: format!("MiniMeters - Audio Scope {i}"),
                vendor: vendors[i % vendors.len()].to_string(),
                format: if i % 3 == 0 {
                    PluginFormat::Clap
                } else {
                    PluginFormat::Vst3
                },
                category: cats[i % cats.len()].to_string(),
                raw_category: Some("Fx|Analyzer".to_string()),
                sub_categories: Some("Fx|Analyzer".to_string()),
                kind: if i % 7 == 0 {
                    PluginKind::Instrument
                } else {
                    PluginKind::Effect
                },
                path: PathBuf::from(format!("C:/Plugins/Plugin{i}.vst3")),
                class_id: Some(format!("com.vendor.plugin{i}")),
                version: Some("1.0.0".to_string()),
                sdk_metadata_loaded: true,
                preset_path: PathBuf::from(format!("C:/Cache/{i}.pst")),
                scanned_at_ms: 0,
                status: PluginStatus::PresetReady,
                scan_status: PluginScanStatus::Success,
                error_message: None,
            })
            .collect()
    }

    // Run with: cargo test -p sphere_ui_components picker_filter_cost -- --nocapture
    #[test]
    fn picker_filter_cost() {
        let plugins = synth_plugins(1031);
        let t = Instant::now();
        let index = PluginSearchIndex::from_plugins(plugins);
        eprintln!(
            "[perf-test] index build (1031): {} us",
            t.elapsed().as_micros()
        );

        let t = Instant::now();
        let _clone = index.clone();
        eprintln!(
            "[perf-test] index DEEP clone (old per-keystroke cost): {} us",
            t.elapsed().as_micros()
        );

        let prefs = PluginPickerPrefs::default_with_size();
        let filters = PluginFilterState {
            sidebar: PickerFilter::Effects,
            ..Default::default()
        };

        for q in ["", "eq", "zzzznomatch"] {
            // Average over a few runs to smooth scheduler noise.
            let runs = 20;
            let t = Instant::now();
            let mut last = 0;
            for _ in 0..runs {
                last = compute_filter_result(&index, q, &filters, &prefs, false)
                    .indices
                    .len();
            }
            eprintln!(
                "[perf-test] compute_filter_result q={q:?} results={last} avg={} us ({runs} runs)",
                t.elapsed().as_micros() / runs
            );
        }
    }
}
