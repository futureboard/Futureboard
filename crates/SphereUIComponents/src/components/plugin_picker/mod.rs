//! DAW-grade insert plugin browser overlay.
//!
//! Reads from the cached SQLite catalog only — never scans or loads plug-in
//! binaries from the picker UI thread.

mod category;
mod details;
mod filter;
mod insert;
mod list_view;
mod overlay;
mod prefs;
mod search_index;
mod sidebar;
mod state;

pub use category::{normalize_category, normalized_category_label, NormalizedCategory};
pub use filter::{compute_filter_result, picker_perf_debug, FilterCounts, FilterResult};
pub use insert::{validate_insert, InsertValidation, PluginInsertKind, PluginInsertTarget};
pub use overlay::{
    page_size_for_height, plugin_picker_overlay, plugin_picker_panel, visible_plugin_id_at,
};
pub use prefs::PluginPickerPrefs;
pub use search_index::PluginSearchIndex;
pub use state::{
    CatalogStatus, PickerFilter, PluginFilterState, PluginPickerLoadState,
    PluginPickerScrollHandles, PluginPickerState,
};

use std::sync::Arc;

use gpui::{App, Window};

/// Legacy sentinel rejected by current VST3-only insert creation.
pub const STUB_PLUGIN_ID: &str = "futureboard.stub.gain";

/// Legacy alias preserved for older call sites.
pub const CATEGORY_ALL: &str = "All";

#[derive(Clone)]
pub struct PluginPickerCallbacks {
    pub on_close: Arc<dyn Fn(&(), &mut Window, &mut App) + 'static>,
    pub on_select: Arc<dyn Fn(&String, &mut Window, &mut App) + 'static>,
    pub on_pick: Arc<dyn Fn(&String, &mut Window, &mut App) + 'static>,
    pub on_select_filter: Arc<dyn Fn(&PickerFilter, &mut Window, &mut App) + 'static>,
    pub on_toggle_favorite: Arc<dyn Fn(&String, &mut Window, &mut App) + 'static>,
    pub on_retry_load: Arc<dyn Fn(&(), &mut Window, &mut App) + 'static>,
    pub on_open_plugin_manager: Arc<dyn Fn(&(), &mut Window, &mut App) + 'static>,
    pub on_rebuild_database: Arc<dyn Fn(&(), &mut Window, &mut App) + 'static>,
}

pub fn move_highlight(state: &mut PluginPickerState, delta: isize, visible_len: usize) {
    if visible_len == 0 {
        state.highlighted_index = 0;
        state.selected_id = None;
        return;
    }
    let next = state.highlighted_index as isize + delta;
    state.highlighted_index = next.clamp(0, visible_len as isize - 1) as usize;
}

pub fn sync_selection_from_highlight(
    state: &mut PluginPickerState,
    index: &PluginSearchIndex,
    prefs: &PluginPickerPrefs,
) {
    state.selected_id = visible_plugin_id_at(state, index, prefs);
}

pub fn ensure_default_highlight(
    state: &mut PluginPickerState,
    index: &PluginSearchIndex,
    prefs: &PluginPickerPrefs,
) {
    let result = crate::components::plugin_picker::filter::compute_filter_result(
        index,
        &state.query,
        &state.filters,
        prefs,
        false,
    );
    state.clamp_highlight(result.indices.len());
    if state.selected_id.is_none() && !result.indices.is_empty() {
        // Reuse the pass we just ran rather than recomputing it inside
        // `visible_plugin_id_at` — this is on the per-keystroke path.
        state.selected_id = result
            .indices
            .get(state.highlighted_index)
            .and_then(|&i| index.plugin_at(i))
            .map(|plugin| plugin.id.clone());
    }
}
