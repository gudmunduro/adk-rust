// Re-export Event and EventActions from adk_core for unified type
pub use adk_core::{Event, EventActions};

/// Trait for accessing events in a session.
pub trait Events: Send + Sync {
    /// Returns all events in the session.
    fn all(&self) -> Vec<Event>;
    /// Returns the number of events in the session.
    fn len(&self) -> usize;
    /// Returns the event at the given index, or `None` if out of bounds.
    fn at(&self, index: usize) -> Option<&Event>;
    /// Returns `true` if the session has no events.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
