//! Plugin picker state, filters, and catalog load status.

use crate::components::plugin_picker::insert::PluginInsertTarget;
use crate::components::timeline::timeline_state::TrackType;
use sphere_plugin_host::PluginFormat;

/// Sidebar filter rail — composes with search query and optional secondary filters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerFilter {
    All,
    Favorites,
    RecentlyUsed,
    Instruments,
    Effects,
    Format(PluginFormat),
    Vendor(String),
    Category(String),
    Failed,
}

impl Default for PickerFilter {
    fn default() -> Self {
        Self::All
    }
}

/// Multi-dimensional filter state applied together with the search query.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginFilterState {
    pub sidebar: PickerFilter,
    pub format: Option<PluginFormat>,
    pub vendor: Option<String>,
    pub category: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PluginPickerState {
    pub is_open: bool,
    pub insert_target: PluginInsertTarget,
    pub filters: PluginFilterState,
    pub query: String,
    pub selected_id: Option<String>,
    pub highlighted_index: usize,
    pub show_details: bool,
}

impl PluginPickerState {
    pub fn closed() -> Self {
        Self {
            is_open: false,
            insert_target: PluginInsertTarget {
                track_id: String::new(),
                track_name: String::new(),
                track_type: TrackType::Audio,
                next_slot_index: 0,
            },
            filters: PluginFilterState::default(),
            query: String::new(),
            selected_id: None,
            highlighted_index: 0,
            show_details: true,
        }
    }

    pub fn open_for(
        track_id: &str,
        track_name: &str,
        track_type: TrackType,
        next_slot_index: usize,
        show_details: bool,
    ) -> Self {
        Self::open_for_with_filter(
            track_id,
            track_name,
            track_type,
            next_slot_index,
            show_details,
            PickerFilter::All,
        )
    }

    pub fn open_for_with_filter(
        track_id: &str,
        track_name: &str,
        track_type: TrackType,
        next_slot_index: usize,
        show_details: bool,
        sidebar_filter: PickerFilter,
    ) -> Self {
        Self {
            is_open: true,
            insert_target: PluginInsertTarget {
                track_id: track_id.to_string(),
                track_name: track_name.to_string(),
                track_type,
                next_slot_index,
            },
            filters: PluginFilterState {
                sidebar: sidebar_filter,
                ..PluginFilterState::default()
            },
            query: String::new(),
            selected_id: None,
            highlighted_index: 0,
            show_details,
        }
    }

    pub fn reset_selection_for_filter_change(&mut self) {
        self.highlighted_index = 0;
        self.selected_id = None;
    }

    pub fn clamp_highlight(&mut self, visible_len: usize) {
        if visible_len == 0 {
            self.highlighted_index = 0;
            self.selected_id = None;
            return;
        }
        if self.highlighted_index >= visible_len {
            self.highlighted_index = visible_len - 1;
        }
    }

    pub fn set_sidebar_filter(&mut self, filter: PickerFilter) {
        self.filters.sidebar = filter;
        self.reset_selection_for_filter_change();
    }
}

/// Loading / error state for the cached catalog.
#[derive(Debug, Clone)]
pub enum CatalogStatus {
    Loading,
    Ready,
    MissingDatabase,
    Error(String),
}

pub type PluginPickerLoadState = CatalogStatus;
