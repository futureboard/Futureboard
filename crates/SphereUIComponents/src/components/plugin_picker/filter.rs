//! Filter counts and multi-filter matching.

use std::collections::{BTreeMap, BTreeSet};

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

pub fn compute_filter_result(
    index: &PluginSearchIndex,
    query: &str,
    filters: &PluginFilterState,
    prefs: &PluginPickerPrefs,
    debug_mode: bool,
) -> FilterResult {
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
    let mut vendor_set = BTreeSet::new();
    let mut category_set = BTreeSet::new();
    let mut indices = Vec::new();

    for (idx, plugin) in plugins.iter().enumerate() {
        update_counts(&mut counts, plugin, prefs, debug_mode);
        if !plugin.vendor.is_empty() {
            vendor_set.insert(plugin.vendor.clone());
        }
        category_set.insert(index.category(idx).to_string());

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

    FilterResult {
        indices,
        counts,
        vendors: vendor_set.into_iter().take(48).collect(),
        categories: category_set.into_iter().take(48).collect(),
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
