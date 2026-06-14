use super::*;

/// Plugin format identifier mirrored from `project::PluginFormat`. Kept
/// in the UI state so we can render an icon/badge without depending on
/// the project crate from render code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertPluginFormat {
    Vst3,
    Clap,
    Au,
    Lv2,
    Unknown,
}

impl InsertPluginFormat {
    pub fn label(self) -> &'static str {
        match self {
            InsertPluginFormat::Vst3 => "VST3",
            InsertPluginFormat::Clap => "CLAP",
            InsertPluginFormat::Au => "AU",
            InsertPluginFormat::Lv2 => "LV2",
            InsertPluginFormat::Unknown => "?",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginRuntimeBackend {
    InProcess,
    ExternalBridge,
}

impl PluginRuntimeBackend {
    pub fn label(self) -> &'static str {
        match self {
            PluginRuntimeBackend::InProcess => "in_process",
            PluginRuntimeBackend::ExternalBridge => "external_bridge",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginRuntimeState {
    /// Persisted insert restored; runtime instance not created yet.
    NotLoaded,
    Loading,
    /// DSP instance loaded in host; not yet in audio graph.
    Loaded,
    /// DSP active in audio graph (processing).
    Active,
    /// Legacy alias — treated as Active in UI status.
    Ready,
    EditorOpening,
    EditorOpen,
    /// Editor UI closed; DSP instance still loaded and processing.
    EditorClosed,
    Bypassed,
    /// Plugin binary path no longer resolves.
    Missing(String),
    Failed(String),
    Crashed,
    Unloaded,
}

/// Load progress of an insert slot. Drives the chip color / label.
/// `Loading` is reserved for Phase 2 when actual plugin instantiation
/// runs on a worker thread. Phase 1 transitions Empty → Ready directly
/// because the runtime doesn't yet instantiate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertLoadStatus {
    Empty,
    Loading,
    Ready,
    /// Persisted plugin metadata present but binary not found on disk.
    Missing(String),
    Failed(String),
    Disabled,
}

impl Default for InsertLoadStatus {
    fn default() -> Self {
        InsertLoadStatus::Empty
    }
}

/// Read-only parameter snapshot — populated in Phase 5 by the param
/// event drain pump. Phase 1 keeps the vec empty.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginParameterState {
    pub id: u32,
    pub name: String,
    pub value_normalized: f32,
}

/// UI-side mirror of `project::ProjectInsert`. The runtime owns the
/// actual plugin processor; this struct only stores descriptor +
/// transient UI state (bypass, load status, last-seen parameters).
#[derive(Debug, Clone, PartialEq)]
pub struct InsertSlotState {
    pub id: String,
    /// Stable plugin identifier (`plugin_uid` / classId) — primary key
    /// against the plugin registry. `None` while the slot is empty.
    pub plugin_id: Option<String>,
    pub plugin_path: Option<std::path::PathBuf>,
    pub plugin_format: Option<InsertPluginFormat>,
    /// Display label shown on the mixer strip. "Empty" when no plugin
    /// is loaded; the plugin's `display_name` otherwise.
    pub display_name: String,
    pub enabled: bool,
    pub bypassed: bool,
    pub load_status: InsertLoadStatus,
    pub runtime_backend: PluginRuntimeBackend,
    pub runtime_state: PluginRuntimeState,
    pub host_pid: Option<u32>,
    pub parameters: Vec<PluginParameterState>,
    /// When true, open the plugin editor once runtime reaches Active/Loaded.
    pub pending_open_editor: bool,
    /// Packed VST3 state (`Vst3PluginState::to_packed_bytes`) for project
    /// persistence. Loaded from the project file on open (then pushed to the
    /// plugin host after `LoadPlugin`), refreshed from the host on save.
    /// `Arc` because plugin states can be megabytes and slots are cloned
    /// freely. `None` = no captured state (fresh insert / stateless plugin).
    pub vst3_state: Option<std::sync::Arc<Vec<u8>>>,
}

impl InsertSlotState {
    pub fn empty(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            plugin_id: None,
            plugin_path: None,
            plugin_format: None,
            display_name: "Empty".to_string(),
            enabled: true,
            bypassed: false,
            load_status: InsertLoadStatus::Empty,
            runtime_backend: PluginRuntimeBackend::InProcess,
            runtime_state: PluginRuntimeState::Unloaded,
            host_pid: None,
            parameters: Vec::new(),
            pending_open_editor: false,
            vst3_state: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.plugin_id.is_none()
    }
}

pub const MASTER_TRACK_ID: &str = "master";

impl TimelineState {
    pub fn insert_slots(&self, track_id: &str) -> Option<&Vec<InsertSlotState>> {
        if track_id == MASTER_TRACK_ID {
            Some(&self.master.inserts)
        } else {
            self.tracks
                .iter()
                .find(|track| track.id == track_id)
                .map(|track| &track.inserts)
        }
    }

    pub fn insert_slots_mut(&mut self, track_id: &str) -> Option<&mut Vec<InsertSlotState>> {
        if track_id == MASTER_TRACK_ID {
            Some(&mut self.master.inserts)
        } else {
            self.tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .map(|track| &mut track.inserts)
        }
    }

    pub fn insert_slot_at(&self, track_id: &str, slot_index: usize) -> Option<&InsertSlotState> {
        self.insert_slots(track_id)
            .and_then(|slots| slots.get(slot_index))
    }

    pub fn find_insert_slot(&self, track_id: &str, insert_id: &str) -> Option<&InsertSlotState> {
        self.insert_slots(track_id)?
            .iter()
            .find(|slot| slot.id == insert_id)
    }

    pub fn insert_owner_ids_containing(&self, insert_id: &str) -> Vec<String> {
        let mut owners: Vec<String> = self
            .tracks
            .iter()
            .filter(|track| track.inserts.iter().any(|slot| slot.id == insert_id))
            .map(|track| track.id.clone())
            .collect();
        if self.master.inserts.iter().any(|slot| slot.id == insert_id) {
            owners.push(MASTER_TRACK_ID.to_string());
        }
        owners
    }

    /// Append an empty insert slot to a track and return the slot id.
    /// Phase 1 — purely UI state; runtime is updated on the next project
    /// sync (the engine ignores unknown plugin descriptors gracefully).
    pub fn add_insert(&mut self, track_id: &str) -> Option<String> {
        let slots = self.insert_slots_mut(track_id)?;
        let slot_id = Self::next_insert_slot_id_for(track_id, slots);
        let slot = InsertSlotState::empty(&slot_id);
        if plugin_debug_enabled() {
            eprintln!("[plugin] add_insert track={} slot_id={}", track_id, slot_id);
        }
        slots.push(slot);
        crate::forensic_trace::log_trace_plugin(track_id, &slot_id);
        Some(slot_id)
    }

    pub fn ensure_insert_slot_at(&mut self, track_id: &str, slot_index: usize) -> Option<String> {
        let slots = self.insert_slots_mut(track_id)?;
        while slots.len() <= slot_index {
            let slot_id = Self::next_insert_slot_id_for(track_id, slots);
            if plugin_debug_enabled() {
                eprintln!("[plugin] add_insert track={} slot_id={}", track_id, slot_id);
            }
            slots.push(InsertSlotState::empty(&slot_id));
        }
        slots.get(slot_index).map(|slot| slot.id.clone())
    }

    fn next_insert_slot_id_for(owner_id: &str, slots: &[InsertSlotState]) -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_INSERT_SLOT_SEQ: AtomicU64 = AtomicU64::new(1);
        // Session-monotonic so a removed slot's id is NEVER regenerated. The
        // audio engine reconciles live VST3 instances by insert id (plugin
        // path/class_id are only an extra reuse guard), so handing a fresh slot
        // the id of a just-removed one resurrects the old instance — the exact
        // "old VSTi is still there" bug. The counter is process-global; we still
        // verify against the track's current ids so a fresh id can never collide
        // with one loaded from a saved project (whose suffixes are arbitrary).
        loop {
            let seq = NEXT_INSERT_SLOT_SEQ.fetch_add(1, Ordering::Relaxed);
            let candidate = format!("insert-{}-{}", owner_id, seq);
            if slots.iter().all(|slot| slot.id != candidate) {
                return candidate;
            }
        }
    }

    /// Assign a plugin to an insert slot. The caller resolves the
    /// `plugin_id` → display metadata before calling so the UI doesn't
    /// have to know about the plugin registry directly.
    pub fn set_insert_plugin(
        &mut self,
        track_id: &str,
        insert_id: &str,
        plugin_id: String,
        plugin_path: Option<std::path::PathBuf>,
        plugin_format: InsertPluginFormat,
        display_name: String,
    ) {
        if track_id == MASTER_TRACK_ID {
            let Some(slot) = self.master.inserts.iter_mut().find(|i| i.id == insert_id) else {
                return;
            };
            slot.plugin_id = Some(plugin_id);
            slot.plugin_path = plugin_path;
            slot.plugin_format = Some(plugin_format);
            slot.display_name = display_name;
            slot.load_status = InsertLoadStatus::Ready;
            slot.runtime_backend = PluginRuntimeBackend::InProcess;
            slot.runtime_state = PluginRuntimeState::Ready;
            slot.host_pid = None;
            slot.bypassed = false;
            slot.parameters.clear();
            crate::forensic_trace::log_trace_plugin(track_id, insert_id);
            if plugin_debug_enabled() {
                eprintln!(
                    "[plugin] set_insert_plugin track={} slot={} -> {}",
                    track_id, insert_id, slot.display_name
                );
            }
            return;
        }

        let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) else {
            return;
        };
        let is_instrument_slot =
            matches!(track.track_type, TrackType::Instrument | TrackType::Midi)
                && track
                    .inserts
                    .first()
                    .map(|first| first.id == insert_id)
                    .unwrap_or(false);
        let Some(slot) = track.inserts.iter_mut().find(|i| i.id == insert_id) else {
            return;
        };
        slot.plugin_id = Some(plugin_id);
        slot.plugin_path = plugin_path;
        slot.plugin_format = Some(plugin_format);
        slot.display_name = display_name;
        slot.load_status = InsertLoadStatus::Ready;
        slot.runtime_backend = PluginRuntimeBackend::InProcess;
        slot.runtime_state = PluginRuntimeState::Ready;
        slot.host_pid = None;
        slot.bypassed = false;
        slot.parameters.clear();
        crate::forensic_trace::log_trace_plugin(track_id, insert_id);
        if is_instrument_slot {
            track.instrument_plugin_instance_id = Some(insert_id.to_string());
            eprintln!("[instrument-route] track={track_id} instrument_instance={insert_id}");
            eprintln!("[instrument-route] plugin_instance_id={insert_id}");
            eprintln!("[instrument-route] route_ok=true");
        }
        if plugin_debug_enabled() {
            eprintln!(
                "[plugin] set_insert_plugin track={} slot={} -> {}",
                track_id, insert_id, slot.display_name
            );
        }
    }

    pub fn set_insert_runtime(
        &mut self,
        track_id: &str,
        insert_id: &str,
        backend: PluginRuntimeBackend,
        state: PluginRuntimeState,
        host_pid: Option<u32>,
    ) -> bool {
        let Some(slots) = self.insert_slots_mut(track_id) else {
            return false;
        };
        let Some(slot) = slots.iter_mut().find(|i| i.id == insert_id) else {
            return false;
        };
        let status = match &state {
            PluginRuntimeState::NotLoaded | PluginRuntimeState::Unloaded => {
                InsertLoadStatus::Loading
            }
            PluginRuntimeState::Loading | PluginRuntimeState::EditorOpening => {
                InsertLoadStatus::Loading
            }
            PluginRuntimeState::Loaded
            | PluginRuntimeState::Active
            | PluginRuntimeState::Ready
            | PluginRuntimeState::EditorOpen
            | PluginRuntimeState::EditorClosed
            | PluginRuntimeState::Bypassed => InsertLoadStatus::Ready,
            PluginRuntimeState::Missing(message) => InsertLoadStatus::Missing(message.clone()),
            PluginRuntimeState::Failed(message) => InsertLoadStatus::Failed(message.clone()),
            PluginRuntimeState::Crashed => {
                InsertLoadStatus::Failed("Plugin host crashed".to_string())
            }
        };
        let changed = slot.runtime_backend != backend
            || slot.runtime_state != state
            || slot.host_pid != host_pid
            || slot.load_status != status;
        slot.runtime_backend = backend;
        slot.runtime_state = state;
        slot.host_pid = host_pid;
        slot.load_status = status;
        changed
    }

    pub fn remove_insert(&mut self, track_id: &str, insert_id: &str) {
        if track_id == MASTER_TRACK_ID {
            let was_present = self.master.inserts.iter().any(|i| i.id == insert_id);
            self.master.inserts.retain(|i| i.id != insert_id);
            if was_present {
                eprintln!("[PluginUnload] model remove_insert track={track_id} slot={insert_id}");
            }
            if plugin_debug_enabled() {
                eprintln!(
                    "[plugin] remove_insert track={} slot={}",
                    track_id, insert_id
                );
            }
            return;
        }

        let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) else {
            return;
        };
        let was_present = track.inserts.iter().any(|i| i.id == insert_id);
        track.inserts.retain(|i| i.id != insert_id);
        // Drop automation lanes bound to the removed instance's parameters — they
        // would otherwise reference a destroyed PluginInstanceId, and a re-add
        // gets a fresh id so they can never re-bind. (Automation bindings don't
        // keep the plugin alive, but stale lanes are dead weight + confusing.)
        track.automation_lanes.retain(|lane| {
            !matches!(&lane.target, AutomationTarget::PluginParameter { insert_id: id, .. } if id == insert_id)
        });
        // If the removed slot was this track's canonical MIDI/instrument
        // destination, drop the dangling pointer. The instrument insert is
        // always `inserts[0]` on an Instrument/MIDI track, so re-point it at the
        // new first non-empty slot, or clear it when none remains. Without this
        // the track keeps routing MIDI to a destroyed PluginInstanceId.
        if track.instrument_plugin_instance_id.as_deref() == Some(insert_id) {
            track.instrument_plugin_instance_id = track
                .inserts
                .first()
                .filter(|slot| !slot.is_empty())
                .map(|slot| slot.id.clone());
        }
        if was_present {
            eprintln!(
                "[PluginUnload] model remove_insert track={} slot={} instrument_instance={:?}",
                track_id, insert_id, track.instrument_plugin_instance_id
            );
        }
        if plugin_debug_enabled() {
            eprintln!(
                "[plugin] remove_insert track={} slot={}",
                track_id, insert_id
            );
        }
    }

    /// Replace the insert slot identified by `old_insert_id` with a brand-new
    /// empty slot at the SAME index, returning the fresh slot id. The replace
    /// flow uses this so a replaced plugin never inherits the previous instance
    /// id — the engine reconciles live VST3 instances by insert id, so reusing
    /// the id would resurrect the old instance (and reloading the same plugin
    /// file must produce an independent instance). Returns `None` if the slot is
    /// not found. The caller is responsible for tearing the OLD instance down
    /// (editor / bridge host / engine sink) before calling this.
    pub fn replace_insert_with_fresh_slot(
        &mut self,
        track_id: &str,
        old_insert_id: &str,
    ) -> Option<String> {
        if track_id == MASTER_TRACK_ID {
            let idx = self
                .master
                .inserts
                .iter()
                .position(|s| s.id == old_insert_id)?;
            let fresh_id = Self::next_insert_slot_id_for(track_id, &self.master.inserts);
            self.master.inserts[idx] = InsertSlotState::empty(&fresh_id);
            eprintln!(
                "[PluginAdd] replace_insert_with_fresh_slot track={track_id} old={old_insert_id} new={fresh_id}"
            );
            return Some(fresh_id);
        }

        let track = self.tracks.iter_mut().find(|t| t.id == track_id)?;
        let idx = track.inserts.iter().position(|s| s.id == old_insert_id)?;
        let fresh_id = Self::next_insert_slot_id_for(track_id, &track.inserts);
        track.inserts[idx] = InsertSlotState::empty(&fresh_id);
        // Drop automation lanes bound to the OLD instance so no old state leaks
        // into the fresh instance (which gets a brand-new id below).
        track.automation_lanes.retain(|lane| {
            !matches!(&lane.target, AutomationTarget::PluginParameter { insert_id: id, .. } if id == old_insert_id)
        });
        // Drop the dangling instrument pointer; `set_insert_plugin` re-points it
        // at the fresh id when the new plugin binds.
        if track.instrument_plugin_instance_id.as_deref() == Some(old_insert_id) {
            track.instrument_plugin_instance_id = None;
        }
        eprintln!(
            "[PluginAdd] replace_insert_with_fresh_slot track={track_id} old={old_insert_id} new={fresh_id}"
        );
        Some(fresh_id)
    }

    /// Move an insert slot one position earlier (`up = true`) or later within
    /// the track's chain. Returns `true` if the order changed. Reordering the
    /// `Vec` is sufficient for the engine — the next project sync carries the
    /// new chain order down to the runtime.
    pub fn move_insert(&mut self, track_id: &str, insert_id: &str, up: bool) -> bool {
        let Some(slots) = self.insert_slots_mut(track_id) else {
            return false;
        };
        let Some(idx) = slots.iter().position(|i| i.id == insert_id) else {
            return false;
        };
        let target = if up {
            if idx == 0 {
                return false;
            }
            idx - 1
        } else {
            if idx + 1 >= slots.len() {
                return false;
            }
            idx + 1
        };
        slots.swap(idx, target);
        if plugin_debug_enabled() {
            eprintln!(
                "[plugin] move_insert track={} slot={} {}",
                track_id,
                insert_id,
                if up { "up" } else { "down" }
            );
        }
        true
    }

    /// Current insert-chain order as a list of slot ids (DSP order == UI order).
    pub fn insert_order(&self, track_id: &str) -> Vec<String> {
        self.insert_slots(track_id)
            .map(|slots| slots.iter().map(|slot| slot.id.clone()).collect())
            .unwrap_or_default()
    }

    /// Reorder a track's insert chain so its slot ids match `ordered_ids`.
    /// Slots named in `ordered_ids` are placed in that order; any slot NOT named
    /// (defensive — should never happen for a complete order) is kept at the end
    /// in its current relative order, so no slot is ever dropped.
    ///
    /// The existing [`InsertSlotState`] structs are reordered **in place** —
    /// never recreated — so every per-instance field (bypass, enabled,
    /// `vst3_state`, parameters, runtime state, host pid) follows the plugin
    /// instance across the move. The engine reconciles live VST3 instances by
    /// insert id, so reordering the `Vec` is all the runtime needs: the next
    /// project sync carries the new chain order down without recreating any
    /// instance. Automation lanes target `PluginParameter { insert_id, .. }`, so
    /// they also follow the instance untouched. Returns `true` if order changed.
    pub fn set_insert_order(&mut self, track_id: &str, ordered_ids: &[String]) -> bool {
        let Some(slots) = self.insert_slots_mut(track_id) else {
            return false;
        };
        let before: Vec<String> = slots.iter().map(|slot| slot.id.clone()).collect();
        let mut remaining: Vec<InsertSlotState> = std::mem::take(slots);
        let mut reordered: Vec<InsertSlotState> = Vec::with_capacity(remaining.len());
        for id in ordered_ids {
            if let Some(pos) = remaining.iter().position(|slot| &slot.id == id) {
                reordered.push(remaining.remove(pos));
            }
        }
        // Defensive: keep any slot the caller did not name, in current order.
        reordered.append(&mut remaining);
        let after: Vec<String> = reordered.iter().map(|slot| slot.id.clone()).collect();
        *slots = reordered;
        let changed = before != after;
        if changed && plugin_debug_enabled() {
            eprintln!("[plugin] set_insert_order track={track_id} {before:?} -> {after:?}");
        }
        changed
    }

    /// Pure helper: the insert-id order produced by moving `insert_id` into the
    /// gap at `insertion_index` (0..=len, the position *between* items where the
    /// drop indicator sits). Handles the removal/reinsertion off-by-one so the
    /// item lands exactly at the visual gap regardless of drag direction. Used
    /// by the drag-reorder drop handler to build the `ReorderFxSlot` command's
    /// `after_order`. Returns `current` unchanged if `insert_id` is absent.
    pub fn reordered_insert_ids(
        current: &[String],
        insert_id: &str,
        insertion_index: usize,
    ) -> Vec<String> {
        let mut ids: Vec<String> = current.to_vec();
        let Some(from) = ids.iter().position(|id| id == insert_id) else {
            return ids;
        };
        let id = ids.remove(from);
        // The gap index is in pre-removal coordinates; if the item was before
        // the gap, removing it shifts the gap left by one.
        let mut to = insertion_index;
        if from < to {
            to -= 1;
        }
        let to = to.min(ids.len());
        ids.insert(to, id);
        ids
    }

    pub fn toggle_insert_bypass(&mut self, track_id: &str, insert_id: &str) -> Option<bool> {
        let slot = self
            .insert_slots_mut(track_id)?
            .iter_mut()
            .find(|i| i.id == insert_id)?;
        slot.bypassed = !slot.bypassed;
        if plugin_debug_enabled() {
            eprintln!(
                "[plugin] toggle_bypass track={} slot={} -> {}",
                track_id, insert_id, slot.bypassed
            );
        }
        Some(slot.bypassed)
    }

    pub fn toggle_insert_enabled(&mut self, track_id: &str, insert_id: &str) -> Option<bool> {
        let slot = self
            .insert_slots_mut(track_id)?
            .iter_mut()
            .find(|i| i.id == insert_id)?;
        slot.enabled = !slot.enabled;
        if plugin_debug_enabled() {
            eprintln!(
                "[plugin] toggle_enabled track={} slot={} -> {}",
                track_id, insert_id, slot.enabled
            );
        }
        Some(slot.enabled)
    }

    /// Set an insert slot's load status by id (Phase 2b engine readback).
    /// Returns `true` if the status actually changed, so callers can decide
    /// whether to repaint. Used by the audio sync completion handler to flip
    /// `Failed` when the engine reports a native plugin failed to instantiate.
    pub fn set_insert_load_status(
        &mut self,
        track_id: &str,
        insert_id: &str,
        status: InsertLoadStatus,
    ) -> bool {
        let Some(slots) = self.insert_slots_mut(track_id) else {
            return false;
        };
        let Some(slot) = slots.iter_mut().find(|i| i.id == insert_id) else {
            return false;
        };
        if slot.load_status == status {
            return false;
        }
        if plugin_debug_enabled() {
            eprintln!(
                "[plugin] set_load_status track={} slot={} -> {:?}",
                track_id, insert_id, status
            );
        }
        slot.load_status = status;
        true
    }

    pub fn set_insert_pending_editor_open(
        &mut self,
        track_id: &str,
        insert_id: &str,
        pending: bool,
    ) -> bool {
        let Some(slots) = self.insert_slots_mut(track_id) else {
            return false;
        };
        let Some(slot) = slots.iter_mut().find(|i| i.id == insert_id) else {
            return false;
        };
        if slot.pending_open_editor == pending {
            return false;
        }
        slot.pending_open_editor = pending;
        true
    }

    pub fn take_pending_insert_editor_opens(&mut self) -> Vec<(String, usize, String)> {
        let mut pending = Vec::new();
        for (index, slot) in self.master.inserts.iter_mut().enumerate() {
            if slot.pending_open_editor {
                slot.pending_open_editor = false;
                pending.push((MASTER_TRACK_ID.to_string(), index, slot.id.clone()));
            }
        }
        for track in &mut self.tracks {
            for (index, slot) in track.inserts.iter_mut().enumerate() {
                if slot.pending_open_editor {
                    slot.pending_open_editor = false;
                    pending.push((track.id.clone(), index, slot.id.clone()));
                }
            }
        }
        pending
    }
}
