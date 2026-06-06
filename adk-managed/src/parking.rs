//! Custom tool parking lot for the managed agent runtime.
//!
//! When the agent emits a `custom_tool_use` event, the session loop parks
//! execution until the client delivers a result (or timeout elapses). This
//! module implements that channel-based wait mechanism.
//!
//! # Architecture
//!
//! Internally, each parked tool call gets a `tokio::sync::oneshot` channel.
//! The [`ToolParkingLot::park`] method creates the sender, stores it, and awaits
//! the receiver with a timeout. [`ToolParkingLot::deliver`] looks up the sender
//! and pushes the content through.
//!
//! # Example
//!
//! ```rust,ignore
//! use std::time::Duration;
//! use adk_managed::parking::ToolParkingLot;
//! use adk_managed::types::ContentBlock;
//!
//! let lot = ToolParkingLot::new(Duration::from_secs(300));
//!
//! // In the session loop task:
//! let result = lot.park("tool_use_abc").await?;
//!
//! // In the event dispatch task:
//! lot.deliver("tool_use_abc", vec![ContentBlock::Text { text: "done".into() }]).await?;
//! ```

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::{Mutex, oneshot};

use crate::types::{ContentBlock, RuntimeError};

/// Default timeout for parked tool calls (5 minutes).
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

/// A parking lot for custom tool calls awaiting client-delivered results.
///
/// The session loop calls [`park`](Self::park) when `agent.custom_tool_use` is emitted,
/// blocking until either:
/// 1. The client sends `user.custom_tool_result` and the runtime calls [`deliver`](Self::deliver)
/// 2. The configured timeout elapses, returning [`RuntimeError::ToolTimeout`]
///
/// Thread-safe: the internal map is protected by a [`Mutex`].
pub struct ToolParkingLot {
    /// Pending tool calls: tool_use_id → sender that will deliver the result.
    pending: Mutex<HashMap<String, oneshot::Sender<Vec<ContentBlock>>>>,
    /// How long to wait before timing out a parked call.
    timeout: Duration,
}

impl ToolParkingLot {
    /// Create a new parking lot with the specified timeout.
    ///
    /// # Arguments
    ///
    /// * `timeout` - Maximum duration to wait for a tool result before returning
    ///   [`RuntimeError::ToolTimeout`].
    pub fn new(timeout: Duration) -> Self {
        Self { pending: Mutex::new(HashMap::new()), timeout }
    }

    /// Create a new parking lot with the default timeout (5 minutes).
    pub fn with_default_timeout() -> Self {
        Self::new(DEFAULT_TIMEOUT)
    }

    /// Park the session loop, waiting for a custom tool result.
    ///
    /// Creates a oneshot channel, stores the sender under `tool_use_id`, and
    /// awaits the receiver. Returns the content blocks when delivered, or
    /// [`RuntimeError::ToolTimeout`] if the timeout elapses.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::ToolTimeout`] if no result is delivered within the timeout.
    /// - [`RuntimeError::Internal`] if the sender is dropped unexpectedly.
    pub async fn park(&self, tool_use_id: &str) -> Result<Vec<ContentBlock>, RuntimeError> {
        let (tx, rx) = oneshot::channel();

        {
            let mut pending = self.pending.lock().await;
            pending.insert(tool_use_id.to_string(), tx);
        }

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(content)) => Ok(content),
            Ok(Err(_recv_error)) => {
                // Sender was dropped without sending — clean up and report internal error.
                let mut pending = self.pending.lock().await;
                pending.remove(tool_use_id);
                Err(RuntimeError::internal(format!(
                    "parking channel closed unexpectedly for tool_use_id: {tool_use_id}"
                )))
            }
            Err(_timeout) => {
                // Timeout elapsed — remove the pending entry and return timeout error.
                let mut pending = self.pending.lock().await;
                pending.remove(tool_use_id);
                Err(RuntimeError::tool_timeout(tool_use_id, self.timeout.as_secs()))
            }
        }
    }

    /// Deliver a result to a parked tool call.
    ///
    /// Looks up the sender by `tool_use_id` and sends the content. Returns an
    /// error if no pending call with this ID exists (e.g., it already timed out
    /// or was never parked).
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::NotFound`] if no pending call exists for the given ID.
    pub async fn deliver(
        &self,
        tool_use_id: &str,
        content: Vec<ContentBlock>,
    ) -> Result<(), RuntimeError> {
        let tx = {
            let mut pending = self.pending.lock().await;
            pending.remove(tool_use_id)
        };

        match tx {
            Some(sender) => {
                // If the receiver was already dropped (e.g., task cancelled), ignore the error.
                let _ = sender.send(content);
                Ok(())
            }
            None => Err(RuntimeError::NotFound {
                session_id: format!("no pending tool call: {tool_use_id}"),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[tokio::test]
    async fn test_successful_park_and_deliver() {
        let lot = Arc::new(ToolParkingLot::new(Duration::from_secs(5)));
        let tool_id = "tool_use_123";

        let lot_clone = Arc::clone(&lot);
        let park_handle = tokio::spawn(async move { lot_clone.park(tool_id).await });

        // Give the park task a moment to register.
        tokio::time::sleep(Duration::from_millis(10)).await;

        let content = vec![ContentBlock::Text { text: "result data".to_string() }];
        lot.deliver(tool_id, content).await.unwrap();

        let result = park_handle.await.unwrap().unwrap();
        assert_eq!(result.len(), 1);
        match &result[0] {
            ContentBlock::Text { text } => assert_eq!(text, "result data"),
            _ => panic!("expected Text variant"),
        }
    }

    #[tokio::test]
    async fn test_timeout_returns_tool_timeout_error() {
        let lot = ToolParkingLot::new(Duration::from_millis(50));
        let tool_id = "tool_use_timeout";

        let result = lot.park(tool_id).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            RuntimeError::ToolTimeout { tool_use_id, timeout_secs } => {
                assert_eq!(tool_use_id, tool_id);
                // Duration is 50ms, which rounds down to 0 seconds.
                assert_eq!(timeout_secs, 0);
            }
            other => panic!("expected ToolTimeout, got: {other}"),
        }
    }

    #[tokio::test]
    async fn test_deliver_to_unknown_id_returns_not_found() {
        let lot = ToolParkingLot::new(Duration::from_secs(5));

        let result = lot
            .deliver("nonexistent_id", vec![ContentBlock::Text { text: "hello".to_string() }])
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            RuntimeError::NotFound { session_id } => {
                assert!(session_id.contains("nonexistent_id"));
            }
            other => panic!("expected NotFound, got: {other}"),
        }
    }

    #[tokio::test]
    async fn test_default_timeout_is_five_minutes() {
        let lot = ToolParkingLot::with_default_timeout();
        assert_eq!(lot.timeout, Duration::from_secs(300));
    }

    #[tokio::test]
    async fn test_multiple_concurrent_parks() {
        let lot = Arc::new(ToolParkingLot::new(Duration::from_secs(5)));

        let lot_a = Arc::clone(&lot);
        let handle_a = tokio::spawn(async move { lot_a.park("tool_a").await });

        let lot_b = Arc::clone(&lot);
        let handle_b = tokio::spawn(async move { lot_b.park("tool_b").await });

        tokio::time::sleep(Duration::from_millis(10)).await;

        lot.deliver("tool_b", vec![ContentBlock::Text { text: "b_result".to_string() }])
            .await
            .unwrap();

        lot.deliver("tool_a", vec![ContentBlock::Text { text: "a_result".to_string() }])
            .await
            .unwrap();

        let result_a = handle_a.await.unwrap().unwrap();
        let result_b = handle_b.await.unwrap().unwrap();

        match &result_a[0] {
            ContentBlock::Text { text } => assert_eq!(text, "a_result"),
            _ => panic!("expected Text"),
        }
        match &result_b[0] {
            ContentBlock::Text { text } => assert_eq!(text, "b_result"),
            _ => panic!("expected Text"),
        }
    }

    #[tokio::test]
    async fn test_deliver_after_timeout_returns_not_found() {
        let lot = ToolParkingLot::new(Duration::from_millis(20));
        let tool_id = "tool_expired";

        // Park and let it timeout.
        let _ = lot.park(tool_id).await;

        // Now try to deliver — should fail because the entry was cleaned up.
        let result =
            lot.deliver(tool_id, vec![ContentBlock::Text { text: "late".to_string() }]).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            RuntimeError::NotFound { .. } => {}
            other => panic!("expected NotFound, got: {other}"),
        }
    }
}
