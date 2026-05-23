//! Structured audit logging interceptor for A2A requests.
//!
//! This module provides an [`AuditLogInterceptor`] that emits structured
//! [`tracing`] events for all A2A requests, capturing the method, client identity,
//! request duration, and optionally the response body.
//!
//! # Architecture
//!
//! The interceptor uses the metadata map on [`A2aDelegationContext`] to store the
//! request start time (as nanoseconds since an arbitrary epoch). In `before_delegation`,
//! it records the start time and emits an info-level event. In `after_delegation`, it
//! calculates the elapsed duration and emits a completion event.
//!
//! # Example
//!
//! ```rust
//! use adk_server::a2a::audit_log::AuditLogInterceptor;
//! use adk_server::a2a::interceptor::{
//!     A2aDelegationContext, A2aInterceptor, InterceptorChain, InterceptorDecision,
//! };
//! use std::collections::HashMap;
//!
//! # tokio_test::block_on(async {
//! // Create an audit log interceptor that does not include response bodies
//! let audit = AuditLogInterceptor::new(false);
//! let chain = InterceptorChain::new().add(audit);
//!
//! let mut ctx = A2aDelegationContext {
//!     method: "tasks/send".to_string(),
//!     params: serde_json::json!({"message": "hello"}),
//!     caller_id: Some("agent-42".to_string()),
//!     metadata: HashMap::new(),
//! };
//!
//! // before_delegation emits a tracing::info! event and stores start time
//! let decision = chain.run_before(&mut ctx).await.unwrap();
//! assert!(matches!(decision, InterceptorDecision::Continue));
//!
//! // Verify start time was stored in metadata
//! assert!(ctx.metadata.contains_key("__audit_start_ns"));
//!
//! // after_delegation emits a completion event with duration
//! let mut response = serde_json::json!({"result": "ok"});
//! chain.run_after(&ctx, &mut response).await.unwrap();
//! # });
//! ```

use async_trait::async_trait;

use super::interceptor::{A2aDelegationContext, A2aError, A2aInterceptor, InterceptorDecision};

/// Metadata key used to store the request start time (nanoseconds from [`Instant`]).
///
/// This is an internal implementation detail and should not be relied upon by
/// external code.
const AUDIT_START_KEY: &str = "__audit_start_ns";

/// A2A interceptor that emits structured tracing events for all requests.
///
/// Logs the method, caller identity, and request duration as structured fields
/// on `tracing::info!` events. Optionally includes the response body in the
/// completion event for debugging purposes.
///
/// # Fields Emitted
///
/// ## `before_delegation` event
/// - `method` — The JSON-RPC method being invoked.
/// - `caller_id` — The caller identity (or `"anonymous"` if not set).
///
/// ## `after_delegation` event
/// - `method` — The JSON-RPC method that was invoked.
/// - `caller_id` — The caller identity (or `"anonymous"` if not set).
/// - `duration_ms` — The elapsed time in milliseconds since `before_delegation`.
/// - `response` (optional) — The JSON response body, included only when
///   [`include_response`](AuditLogInterceptor::include_response) is `true`.
///
/// # Example
///
/// ```rust
/// use adk_server::a2a::audit_log::AuditLogInterceptor;
/// use adk_server::a2a::interceptor::{
///     A2aDelegationContext, A2aInterceptor, InterceptorDecision,
/// };
/// use std::collections::HashMap;
///
/// # tokio_test::block_on(async {
/// // Include response bodies in audit logs (useful for debugging)
/// let audit = AuditLogInterceptor::new(true);
///
/// let mut ctx = A2aDelegationContext {
///     method: "tasks/get".to_string(),
///     params: serde_json::json!({}),
///     caller_id: Some("client-abc".to_string()),
///     metadata: HashMap::new(),
/// };
///
/// let decision = audit.before_delegation(&mut ctx).await.unwrap();
/// assert!(matches!(decision, InterceptorDecision::Continue));
///
/// let mut response = serde_json::json!({"status": "completed"});
/// audit.after_delegation(&ctx, &mut response).await.unwrap();
/// # });
/// ```
#[derive(Debug, Clone)]
pub struct AuditLogInterceptor {
    /// Whether to include the response body in the audit log completion event.
    pub include_response: bool,
}

impl AuditLogInterceptor {
    /// Creates a new `AuditLogInterceptor`.
    ///
    /// # Arguments
    ///
    /// * `include_response` — When `true`, the response body is included in the
    ///   `after_delegation` tracing event. Set to `false` in production to avoid
    ///   logging sensitive data.
    ///
    /// # Example
    ///
    /// ```rust
    /// use adk_server::a2a::audit_log::AuditLogInterceptor;
    ///
    /// // Production: don't log response bodies
    /// let audit = AuditLogInterceptor::new(false);
    ///
    /// // Development: include response bodies for debugging
    /// let audit_debug = AuditLogInterceptor::new(true);
    /// ```
    pub fn new(include_response: bool) -> Self {
        Self { include_response }
    }

    /// Returns the caller identity string for logging.
    ///
    /// Uses the `caller_id` from the context if available, otherwise returns
    /// `"anonymous"`.
    fn caller_id_or_anonymous(ctx: &A2aDelegationContext) -> &str {
        ctx.caller_id.as_deref().unwrap_or("anonymous")
    }
}

#[async_trait]
impl A2aInterceptor for AuditLogInterceptor {
    /// Emits a structured tracing event with the request method and caller identity,
    /// and stores the start time in the context metadata for duration calculation.
    async fn before_delegation(
        &self,
        ctx: &mut A2aDelegationContext,
    ) -> Result<InterceptorDecision, A2aError> {
        // Store the current timestamp as nanoseconds since UNIX epoch for duration
        // calculation in after_delegation.
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        ctx.metadata.insert(AUDIT_START_KEY.to_string(), now_ns.to_string());

        let caller = Self::caller_id_or_anonymous(ctx);
        tracing::info!(
            method = %ctx.method,
            caller_id = %caller,
            "a2a request started"
        );

        Ok(InterceptorDecision::Continue)
    }

    /// Emits a structured tracing event with the request method, caller identity,
    /// elapsed duration, and optionally the response body.
    async fn after_delegation(
        &self,
        ctx: &A2aDelegationContext,
        response: &mut serde_json::Value,
    ) -> Result<(), A2aError> {
        let duration_ms = ctx
            .metadata
            .get(AUDIT_START_KEY)
            .and_then(|s| s.parse::<u128>().ok())
            .map(|start_ns| {
                let now_ns = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                // Convert nanoseconds difference to milliseconds
                (now_ns.saturating_sub(start_ns)) as f64 / 1_000_000.0
            })
            .unwrap_or(0.0);

        let caller = Self::caller_id_or_anonymous(ctx);

        if self.include_response {
            let response_str = response.to_string();
            tracing::info!(
                method = %ctx.method,
                caller_id = %caller,
                duration_ms = %format!("{duration_ms:.3}"),
                response = %response_str,
                "a2a request completed"
            );
        } else {
            tracing::info!(
                method = %ctx.method,
                caller_id = %caller,
                duration_ms = %format!("{duration_ms:.3}"),
                "a2a request completed"
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_ctx(method: &str, caller_id: Option<&str>) -> A2aDelegationContext {
        A2aDelegationContext {
            method: method.to_string(),
            params: serde_json::json!({}),
            caller_id: caller_id.map(String::from),
            metadata: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_before_delegation_stores_start_time() {
        let audit = AuditLogInterceptor::new(false);
        let mut ctx = make_ctx("tasks/send", Some("agent-1"));

        let decision = audit.before_delegation(&mut ctx).await.unwrap();
        assert!(matches!(decision, InterceptorDecision::Continue));
        assert!(ctx.metadata.contains_key(AUDIT_START_KEY));

        // Verify the stored value is a valid number
        let start_ns: u128 = ctx.metadata[AUDIT_START_KEY].parse().unwrap();
        assert!(start_ns > 0);
    }

    #[tokio::test]
    async fn test_before_delegation_always_continues() {
        let audit = AuditLogInterceptor::new(true);
        let mut ctx = make_ctx("tasks/get", None);

        let decision = audit.before_delegation(&mut ctx).await.unwrap();
        assert!(matches!(decision, InterceptorDecision::Continue));
    }

    #[tokio::test]
    async fn test_after_delegation_succeeds_with_start_time() {
        let audit = AuditLogInterceptor::new(false);
        let mut ctx = make_ctx("tasks/send", Some("client-1"));

        // Simulate before_delegation storing the start time
        audit.before_delegation(&mut ctx).await.unwrap();

        // Small delay to ensure measurable duration
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;

        let mut response = serde_json::json!({"result": "ok"});
        let result = audit.after_delegation(&ctx, &mut response).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_after_delegation_succeeds_without_start_time() {
        let audit = AuditLogInterceptor::new(false);
        let ctx = make_ctx("tasks/send", Some("client-1"));

        // No before_delegation called, so no start time in metadata
        let mut response = serde_json::json!({"result": "ok"});
        let result = audit.after_delegation(&ctx, &mut response).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_anonymous_caller_when_no_caller_id() {
        let audit = AuditLogInterceptor::new(false);
        let mut ctx = make_ctx("tasks/send", None);

        let decision = audit.before_delegation(&mut ctx).await.unwrap();
        assert!(matches!(decision, InterceptorDecision::Continue));

        // Verify the caller_id helper returns "anonymous"
        assert_eq!(AuditLogInterceptor::caller_id_or_anonymous(&ctx), "anonymous");
    }

    #[tokio::test]
    async fn test_include_response_false_does_not_modify_response() {
        let audit = AuditLogInterceptor::new(false);
        let mut ctx = make_ctx("tasks/send", Some("agent-1"));
        audit.before_delegation(&mut ctx).await.unwrap();

        let mut response = serde_json::json!({"result": "sensitive_data"});
        let original = response.clone();
        audit.after_delegation(&ctx, &mut response).await.unwrap();

        // Response should not be modified by the audit interceptor
        assert_eq!(response, original);
    }

    #[tokio::test]
    async fn test_include_response_true_does_not_modify_response() {
        let audit = AuditLogInterceptor::new(true);
        let mut ctx = make_ctx("tasks/send", Some("agent-1"));
        audit.before_delegation(&mut ctx).await.unwrap();

        let mut response = serde_json::json!({"result": "data", "nested": {"key": "value"}});
        let original = response.clone();
        audit.after_delegation(&ctx, &mut response).await.unwrap();

        // Response should not be modified even when include_response is true
        assert_eq!(response, original);
    }

    #[tokio::test]
    async fn test_new_creates_with_include_response() {
        let audit_no_response = AuditLogInterceptor::new(false);
        assert!(!audit_no_response.include_response);

        let audit_with_response = AuditLogInterceptor::new(true);
        assert!(audit_with_response.include_response);
    }

    #[tokio::test]
    async fn test_duration_is_non_negative() {
        let audit = AuditLogInterceptor::new(false);
        let mut ctx = make_ctx("tasks/send", Some("agent-1"));

        audit.before_delegation(&mut ctx).await.unwrap();

        // Verify the start time is a valid timestamp
        let start_ns: u128 = ctx.metadata[AUDIT_START_KEY].parse().unwrap();
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();

        // Duration should be non-negative (now >= start)
        assert!(now_ns >= start_ns);
    }
}
