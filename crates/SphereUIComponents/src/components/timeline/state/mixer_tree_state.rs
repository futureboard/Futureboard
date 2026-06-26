use std::collections::HashSet;

/// Persisted per-project mixer tree UI state (expanded nodes, pins, hidden channels).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MixerTreeViewState {
    pub expanded_node_ids: HashSet<String>,
    pub pinned_channel_ids: HashSet<String>,
    pub hidden_channel_ids: HashSet<String>,
}

impl MixerTreeViewState {
    pub fn is_expanded(&self, node_id: &str) -> bool {
        self.expanded_node_ids.contains(node_id)
    }

    pub fn set_expanded(&mut self, node_id: impl Into<String>, expanded: bool) {
        let node_id = node_id.into();
        if expanded {
            self.expanded_node_ids.insert(node_id);
        } else {
            self.expanded_node_ids.remove(&node_id);
        }
    }

    pub fn expand_all(&mut self, node_ids: impl IntoIterator<Item = String>) {
        self.expanded_node_ids.extend(node_ids);
    }

    pub fn collapse_all(&mut self) {
        self.expanded_node_ids.clear();
    }

    pub fn is_pinned(&self, channel_id: &str) -> bool {
        self.pinned_channel_ids.contains(channel_id)
    }

    pub fn toggle_pin(&mut self, channel_id: &str) -> bool {
        if self.pinned_channel_ids.contains(channel_id) {
            self.pinned_channel_ids.remove(channel_id);
            false
        } else {
            self.pinned_channel_ids.insert(channel_id.to_string());
            true
        }
    }

    pub fn is_channel_hidden(&self, channel_id: &str) -> bool {
        self.hidden_channel_ids.contains(channel_id)
    }

    pub fn toggle_channel_visibility(&mut self, channel_id: &str) -> bool {
        if self.hidden_channel_ids.contains(channel_id) {
            self.hidden_channel_ids.remove(channel_id);
            false
        } else {
            self.hidden_channel_ids.insert(channel_id.to_string());
            true
        }
    }

    pub fn reset_visibility(&mut self) {
        self.hidden_channel_ids.clear();
    }

    pub fn from_project_lists(
        expanded: &[String],
        pinned: &[String],
        hidden: &[String],
    ) -> Self {
        Self {
            expanded_node_ids: expanded.iter().cloned().collect(),
            pinned_channel_ids: pinned.iter().cloned().collect(),
            hidden_channel_ids: hidden.iter().cloned().collect(),
        }
    }

    pub fn expanded_list(&self) -> Vec<String> {
        self.expanded_node_ids.iter().cloned().collect()
    }

    pub fn pinned_list(&self) -> Vec<String> {
        self.pinned_channel_ids.iter().cloned().collect()
    }

    pub fn hidden_list(&self) -> Vec<String> {
        self.hidden_channel_ids.iter().cloned().collect()
    }
}
