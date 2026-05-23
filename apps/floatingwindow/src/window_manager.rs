#![allow(dead_code)]

use std::collections::HashMap;
use crate::protocol::FloatingWindowDescriptor;

/// Tracks which windows have been opened and their descriptors.
/// The actual egui viewport lifecycle is managed in app.rs.
pub struct WindowManager {
    descriptors: HashMap<String, FloatingWindowDescriptor>,
}

impl WindowManager {
    pub fn new() -> Self {
        Self {
            descriptors: HashMap::new(),
        }
    }

    pub fn register(&mut self, desc: FloatingWindowDescriptor) {
        self.descriptors.insert(desc.id.clone(), desc);
    }

    pub fn unregister(&mut self, id: &str) {
        self.descriptors.remove(id);
    }

    pub fn is_open(&self, id: &str) -> bool {
        self.descriptors.contains_key(id)
    }

    pub fn get(&self, id: &str) -> Option<&FloatingWindowDescriptor> {
        self.descriptors.get(id)
    }

    pub fn open_ids(&self) -> Vec<String> {
        self.descriptors.keys().cloned().collect()
    }
}

impl Default for WindowManager {
    fn default() -> Self {
        Self::new()
    }
}
