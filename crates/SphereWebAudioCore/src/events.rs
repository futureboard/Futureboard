//! Engine events — messages from DSP engine to UI.
//!
//! Events are collected in a bounded queue and drained by the adapter
//! on each animation/polling frame.

use serde::{Deserialize, Serialize};

use crate::ids::TrackId;
use crate::meters::MeterSnapshot;


/// Events emitted by the engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EngineEvent {
    Ready,
    Error {
        code: String,
        message: String,
    },
    Warning {
        message: String,
    },
    TransportPosition {
        beat: f64,
        sample: u64,
        time_seconds: f64,
    },
    PlaybackStarted,
    PlaybackStopped,
    PlaybackPaused,
    MeterUpdate {
        meters: Vec<MeterSnapshot>,
    },
    TrackCreated {
        track_id: TrackId,
    },
    TrackRemoved {
        track_id: TrackId,
    },
    Pong,
}

/// Bounded event queue. Oldest events are dropped if capacity is exceeded.
pub struct EventQueue {
    events: Vec<EngineEvent>,
    capacity: usize,
}

impl EventQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            events: Vec::with_capacity(capacity.min(1024)),
            capacity,
        }
    }

    /// Push an event. If at capacity, the oldest event is dropped.
    pub fn push(&mut self, event: EngineEvent) {
        if self.events.len() >= self.capacity {
            self.events.remove(0);
        }
        self.events.push(event);
    }

    /// Drain all pending events.
    pub fn drain(&mut self) -> Vec<EngineEvent> {
        std::mem::take(&mut self.events)
    }

    /// Number of pending events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_queue_drains() {
        let mut q = EventQueue::new(100);
        q.push(EngineEvent::Ready);
        q.push(EngineEvent::Pong);
        assert_eq!(q.len(), 2);
        let events = q.drain();
        assert_eq!(events.len(), 2);
        assert!(q.is_empty());
    }

    #[test]
    fn event_queue_bounded() {
        let mut q = EventQueue::new(3);
        q.push(EngineEvent::Ready);
        q.push(EngineEvent::Pong);
        q.push(EngineEvent::PlaybackStarted);
        q.push(EngineEvent::PlaybackStopped); // Should drop oldest (Ready)
        assert_eq!(q.len(), 3);
        let events = q.drain();
        // First event should be Pong (Ready was dropped)
        assert!(matches!(events[0], EngineEvent::Pong));
    }
}
