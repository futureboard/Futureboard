//! Cached search documents for fast picker filtering.
//!
//! Built once per plugin-scan/catalog load and shared via `Arc`, so typing in
//! the picker never rebuilds it and never re-derives the sidebar vendor/category
//! lists (those are library-wide and query-independent — precomputed here).

use std::collections::BTreeSet;

use SpherePluginHost::RegistryPlugin;

use crate::components::plugin_picker::category::normalized_category_label;

/// Cap on distinct vendors / categories surfaced in the sidebar rail.
const SIDEBAR_FACET_CAP: usize = 48;

#[derive(Debug, Clone)]
pub struct PluginSearchIndex {
    plugins: Vec<RegistryPlugin>,
    search_text: Vec<String>,
    vendors_lower: Vec<String>,
    categories: Vec<String>,
    categories_lower: Vec<String>,
    /// Distinct non-empty vendor labels (original case), sorted + capped. Static
    /// for the library, so the sidebar never recomputes them per keypress.
    sidebar_vendors: Vec<String>,
    /// Distinct normalized category labels, sorted + capped. Static like above.
    sidebar_categories: Vec<String>,
}

impl PluginSearchIndex {
    pub fn from_plugins(plugins: Vec<RegistryPlugin>) -> Self {
        let search_text = plugins.iter().map(build_search_text).collect::<Vec<_>>();
        let categories = plugins
            .iter()
            .map(normalized_category_label)
            .collect::<Vec<_>>();
        let categories_lower = categories
            .iter()
            .map(|category| category.to_ascii_lowercase())
            .collect::<Vec<_>>();
        let vendors_lower = plugins
            .iter()
            .map(|plugin| plugin.vendor.to_ascii_lowercase())
            .collect::<Vec<_>>();
        // Precompute the deduped sidebar facets once (the per-keypress filter
        // path used to rebuild these BTreeSets over every plugin — the bulk of
        // the typing-freeze allocation).
        let mut vendor_set = BTreeSet::new();
        for plugin in &plugins {
            if !plugin.vendor.is_empty() {
                vendor_set.insert(plugin.vendor.clone());
            }
        }
        let sidebar_vendors = vendor_set.into_iter().take(SIDEBAR_FACET_CAP).collect();
        let category_set: BTreeSet<String> = categories.iter().cloned().collect();
        let sidebar_categories = category_set.into_iter().take(SIDEBAR_FACET_CAP).collect();
        Self {
            plugins,
            search_text,
            vendors_lower,
            categories,
            categories_lower,
            sidebar_vendors,
            sidebar_categories,
        }
    }

    pub fn plugins(&self) -> &[RegistryPlugin] {
        &self.plugins
    }

    /// Library-wide distinct vendor labels for the sidebar (precomputed).
    pub fn sidebar_vendors(&self) -> &[String] {
        &self.sidebar_vendors
    }

    /// Library-wide distinct category labels for the sidebar (precomputed).
    pub fn sidebar_categories(&self) -> &[String] {
        &self.sidebar_categories
    }

    pub fn plugin_at(&self, index: usize) -> Option<&RegistryPlugin> {
        self.plugins.get(index)
    }

    pub fn search_text(&self, index: usize) -> &str {
        self.search_text
            .get(index)
            .map(String::as_str)
            .unwrap_or("")
    }

    pub fn vendor_lower(&self, index: usize) -> &str {
        self.vendors_lower
            .get(index)
            .map(String::as_str)
            .unwrap_or("")
    }

    pub fn category(&self, index: usize) -> &str {
        self.categories.get(index).map(String::as_str).unwrap_or("")
    }

    pub fn category_lower(&self, index: usize) -> &str {
        self.categories_lower
            .get(index)
            .map(String::as_str)
            .unwrap_or("")
    }

    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }
}

fn build_search_text(plugin: &RegistryPlugin) -> String {
    let category = normalized_category_label(plugin);
    format!(
        "{} {} {} {} {} {}",
        plugin.name,
        plugin.vendor,
        plugin.display_category(),
        category,
        plugin.format.label(),
        plugin.raw_category.as_deref().unwrap_or(""),
    )
    .to_ascii_lowercase()
}
