use serde_json::Value;
use std::collections::HashMap;

/// Mutable key-value state store for a session.
pub trait State: Send + Sync {
    /// Returns the value for the given key, or `None` if not present.
    fn get(&self, key: &str) -> Option<Value>;
    /// Sets a key-value pair in the state.
    fn set(&mut self, key: String, value: Value);
    /// Returns all key-value pairs in the state.
    fn all(&self) -> HashMap<String, Value>;
}

/// Read-only view of session state.
pub trait ReadonlyState: Send + Sync {
    /// Returns the value for the given key, or `None` if not present.
    fn get(&self, key: &str) -> Option<Value>;
    /// Returns all key-value pairs in the state.
    fn all(&self) -> HashMap<String, Value>;
}
