use chrono::{DateTime, Utc};
use forgemux_core::SessionId;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};

pub const STREAM_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamMessage {
    Resume {
        last_seen_event_id: Option<u64>,
        mode: Option<String>,
        #[serde(default)]
        protocol_version: Option<u32>,
    },
    Event {
        event_id: u64,
        data: String,
        #[serde(default = "default_true")]
        durable: bool,
    },
    Input {
        input_id: String,
        data: String,
    },
    Ack {
        input_id: String,
    },
    Snapshot {
        snapshot_id: u64,
        data: String,
    },
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone)]
pub struct StreamEvent {
    pub id: u64,
    pub data: String,
    pub at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct EventRing {
    capacity: usize,
    events: VecDeque<StreamEvent>,
    next_id: u64,
}

impl EventRing {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            events: VecDeque::new(),
            next_id: 1,
        }
    }

    pub fn push(&mut self, data: String) -> StreamEvent {
        let event = StreamEvent {
            id: self.next_id,
            data,
            at: Utc::now(),
        };
        self.next_id = self.next_id.saturating_add(1);
        self.events.push_back(event.clone());
        while self.events.len() > self.capacity {
            self.events.pop_front();
        }
        event
    }

    pub fn since(&self, last_seen: u64) -> Vec<StreamEvent> {
        self.events
            .iter()
            .filter(|event| event.id > last_seen)
            .cloned()
            .collect()
    }

    pub fn latest_id(&self) -> u64 {
        self.events.back().map(|e| e.id).unwrap_or(0)
    }
}

#[derive(Debug)]
pub struct InputDeduper {
    window: usize,
    order: VecDeque<String>,
    seen: HashSet<String>,
}

impl InputDeduper {
    pub fn new(window: usize) -> Self {
        Self {
            window: window.max(1),
            order: VecDeque::new(),
            seen: HashSet::new(),
        }
    }

    pub fn accept(&mut self, input_id: &str) -> bool {
        if self.seen.contains(input_id) {
            return false;
        }
        self.seen.insert(input_id.to_string());
        self.order.push_back(input_id.to_string());
        while self.order.len() > self.window {
            if let Some(old) = self.order.pop_front() {
                self.seen.remove(&old);
            }
        }
        true
    }
}

#[derive(Debug)]
pub struct StreamState {
    pub ring: EventRing,
    pub deduper: InputDeduper,
    pub last_snapshot: String,
}

#[derive(Clone)]
pub struct StreamManager {
    inner: Arc<Mutex<HashMap<SessionId, StreamState>>>,
    ring_capacity: usize,
    dedup_window: usize,
}

impl StreamManager {
    pub fn new(ring_capacity: usize, dedup_window: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            ring_capacity,
            dedup_window,
        }
    }

    pub fn push_event(&self, session: &SessionId, data: String) -> StreamEvent {
        let mut guard = self.inner.lock().unwrap();
        let state = guard.entry(session.clone()).or_insert_with(|| StreamState {
            ring: EventRing::new(self.ring_capacity),
            deduper: InputDeduper::new(self.dedup_window),
            last_snapshot: String::new(),
        });
        state.ring.push(data)
    }

    pub fn events_since(&self, session: &SessionId, last_seen: u64) -> Vec<StreamEvent> {
        let guard = self.inner.lock().unwrap();
        guard
            .get(session)
            .map(|state| state.ring.since(last_seen))
            .unwrap_or_default()
    }

    pub fn latest_event_id(&self, session: &SessionId) -> u64 {
        let guard = self.inner.lock().unwrap();
        guard
            .get(session)
            .map(|state| state.ring.latest_id())
            .unwrap_or(0)
    }

    pub fn accept_input(&self, session: &SessionId, input_id: &str) -> bool {
        let mut guard = self.inner.lock().unwrap();
        let state = guard.entry(session.clone()).or_insert_with(|| StreamState {
            ring: EventRing::new(self.ring_capacity),
            deduper: InputDeduper::new(self.dedup_window),
            last_snapshot: String::new(),
        });
        state.deduper.accept(input_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_ring_eviction_and_since() {
        let mut ring = EventRing::new(2);
        ring.push("one".to_string());
        let second = ring.push("two".to_string());
        ring.push("three".to_string());

        let events = ring.since(second.id - 1);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data, "two");
        assert_eq!(events[1].data, "three");
    }

    #[test]
    fn input_deduper_rejects_duplicates() {
        let mut deduper = InputDeduper::new(2);
        assert!(deduper.accept("a"));
        assert!(!deduper.accept("a"));
        assert!(deduper.accept("b"));
        assert!(deduper.accept("c"));
        assert!(!deduper.accept("b"));
    }

    #[test]
    fn stream_manager_tracks_events() {
        let manager = StreamManager::new(3, 5);
        let session = SessionId::from("S-1");
        manager.push_event(&session, "hello".to_string());
        manager.push_event(&session, "world".to_string());
        let events = manager.events_since(&session, 0);
        assert_eq!(events.len(), 2);
        assert_eq!(manager.latest_event_id(&session), events[1].id);
    }

    #[test]
    fn event_defaults_to_durable_on_deserialize() {
        let json = r#"{"type":"event","event_id":1,"data":"hi"}"#;
        let msg: StreamMessage = serde_json::from_str(json).unwrap();
        match msg {
            StreamMessage::Event { durable, .. } => assert!(durable),
            _ => panic!("expected event"),
        }
    }

    #[test]
    fn resume_accepts_missing_protocol_version() {
        let json = r#"{"type":"resume","last_seen_event_id":2,"mode":"watch"}"#;
        let msg: StreamMessage = serde_json::from_str(json).unwrap();
        match msg {
            StreamMessage::Resume {
                protocol_version, ..
            } => assert_eq!(protocol_version, None),
            _ => panic!("expected resume"),
        }
    }
}
