//! Cached search documents for fast picker filtering.

use SpherePluginHost::RegistryPlugin;

use crate::components::plugin_picker::category::normalized_category_label;

#[derive(Debug, Clone)]
pub struct PluginSearchIndex {
    plugins: Vec<RegistryPlugin>,
    search_text: Vec<String>,
    vendors_lower: Vec<String>,
    categories: Vec<String>,
    categories_lower: Vec<String>,
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
        Self {
            plugins,
            search_text,
            vendors_lower,
            categories,
            categories_lower,
        }
    }

    pub fn plugins(&self) -> &[RegistryPlugin] {
        &self.plugins
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
