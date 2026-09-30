//! EventBus - Publish/Subscribe event system for decoupled communication
//!
//! The EventBus allows components to communicate without direct dependencies.
//! Subscribers are Python callables; each receives the event payload as a
//! JSON string.

use pyo3::prelude::*;
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;

/// Event types emitted by the engine
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    GraphStart { graph_id: String },
    GraphEnd { graph_id: String },
    NodeEnter { node_id: String, node_type: String },
    BlockCall { node_id: Option<String>, block_type: String, args: JsonValue },
    VariableChanged { name: String, value: JsonValue },
    YieldRequest { reason: String },
    Error { message: String, node_id: Option<String> },
}

impl Event {
    /// Event type name used as the subscription key
    pub fn event_type(&self) -> &'static str {
        match self {
            Event::GraphStart { .. } => "GraphStart",
            Event::GraphEnd { .. } => "GraphEnd",
            Event::NodeEnter { .. } => "NodeEnter",
            Event::BlockCall { .. } => "BlockCall",
            Event::VariableChanged { .. } => "VariableChanged",
            Event::YieldRequest { .. } => "YieldRequest",
            Event::Error { .. } => "Error",
        }
    }

    /// Event payload as a JSON value
    pub fn payload(&self) -> JsonValue {
        match self {
            Event::GraphStart { graph_id } | Event::GraphEnd { graph_id } => {
                json!({ "graph_id": graph_id })
            }
            Event::NodeEnter { node_id, node_type } => {
                json!({ "node_id": node_id, "node_type": node_type })
            }
            Event::BlockCall { node_id, block_type, args } => {
                json!({ "node_id": node_id, "block_type": block_type, "args": args })
            }
            Event::VariableChanged { name, value } => json!({ "name": name, "value": value }),
            Event::YieldRequest { reason } => json!({ "reason": reason }),
            Event::Error { message, node_id } => json!({ "message": message, "node_id": node_id }),
        }
    }
}

/// EventBus manages pub/sub communication between engine components
#[pyclass]
pub struct EventBus {
    subscribers: HashMap<String, Vec<Py<PyAny>>>,
}

#[pymethods]
impl EventBus {
    #[new]
    pub fn new() -> Self {
        EventBus {
            subscribers: HashMap::new(),
        }
    }

    /// Subscribe a callable to an event type. Use "*" to receive every event;
    /// wildcard subscribers are called with (event_type, payload).
    pub fn subscribe(&mut self, event_type: &str, callback: Py<PyAny>) {
        self.subscribers
            .entry(event_type.to_string())
            .or_default()
            .push(callback);
        log::info!("Subscribed to event type: {}", event_type);
    }

    /// Publish an event to all subscribers. The payload is passed as-is
    /// (the engine always sends a JSON string).
    pub fn publish(slf: &Bound<'_, Self>, event_type: &str, payload: &str) -> PyResult<()> {
        log::debug!("Event [{}]: {}", event_type, payload);
        let py = slf.py();
        // Snapshot the callbacks and release the borrow, so a callback may
        // subscribe or publish without a borrow conflict.
        let (direct, wildcard): (Vec<Py<PyAny>>, Vec<Py<PyAny>>) = {
            let bus = slf.borrow();
            let get = |key: &str| -> Vec<Py<PyAny>> {
                bus.subscribers
                    .get(key)
                    .map(|v| v.iter().map(|c| c.clone_ref(py)).collect())
                    .unwrap_or_default()
            };
            (get(event_type), get("*"))
        };
        for callback in direct {
            callback.call1(py, (payload,))?;
        }
        for callback in wildcard {
            callback.call1(py, (event_type, payload))?;
        }
        Ok(())
    }

    /// Clear all subscriptions
    pub fn clear(&mut self) {
        self.subscribers.clear();
    }

    /// Get subscriber count for an event type
    pub fn subscriber_count(&self, event_type: &str) -> usize {
        self.subscribers.get(event_type).map(|v| v.len()).unwrap_or(0)
    }
}

impl EventBus {
    /// Publish a typed engine event
    pub fn emit(slf: &Bound<'_, Self>, event: &Event) -> PyResult<()> {
        Self::publish(slf, event.event_type(), &event.payload().to_string())
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
