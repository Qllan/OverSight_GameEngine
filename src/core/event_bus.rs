//! EventBus - Publish/Subscribe event system for decoupled communication
//!
//! The EventBus allows components to communicate without direct dependencies.
//! Events are strongly typed and can be subscribed to by multiple listeners.

use serde_json::Value as JsonValue;
use std::collections::HashMap;
use pyo3::prelude::*;

/// Event types emitted by the engine
#[derive(Debug, Clone)]
#[pyclass]
pub enum Event {
    GraphStart { graph_id: String },
    GraphEnd { graph_id: String },
    NodeEnter { node_id: String, node_type: String },
    NodeExit { node_id: String },
    VariableChanged { name: String, value: JsonValue },
    YieldRequest { reason: String },
    Error { message: String, node_id: Option<String> },
}

/// Callback type for event subscribers
type EventCallback = Box<dyn Fn(&Event) + Send + Sync>;

/// EventBus manages pub/sub communication between engine components
#[pyclass]
pub struct EventBus {
    subscribers: HashMap<String, Vec<EventCallback>>,
}

#[pymethods]
impl EventBus {
    #[new]
    fn new() -> Self {
        EventBus {
            subscribers: HashMap::new(),
        }
    }

    /// Subscribe to an event type
    /// Note: Python callbacks are not fully supported in Phase 1 (stub)
    fn subscribe(&mut self, event_type: &str) {
        self.subscribers.entry(event_type.to_string()).or_default();
        log::info!("Subscribed to event type: {}", event_type);
    }

    /// Publish an event to all subscribers
    fn publish(&self, event_type: &str, message: &str) {
        log::debug!("Event [{}]: {}", event_type, message);
        if let Some(callbacks) = self.subscribers.get(event_type) {
            for callback in callbacks {
                // In Phase 1, we just log; full callback support in Phase 2
                let _ = callback;
            }
        }
    }

    /// Clear all subscriptions
    fn clear(&mut self) {
        self.subscribers.clear();
    }

    /// Get subscriber count for an event type
    fn subscriber_count(&self, event_type: &str) -> usize {
        self.subscribers.get(event_type).map(|v| v.len()).unwrap_or(0)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
