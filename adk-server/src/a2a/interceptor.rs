//! A2A interceptor framework for request/response middleware.
//!
//! This module provides an interceptor chain that can inspect, modify, or reject
//! A2A requests before and after delegation to the executor. Interceptors enable
//! cross-cutting concerns like authentication, rate limiting, and audit logging.
//!
//! # Architecture
//!
//! Interceptors execute in registration order for `before_delegation` (first registered
//! runs first) and in reverse order for `after_delegation` (last registered runs first).
//! This follows the standard middleware onion model.
//!
//! # Example
//!
//! ```rust
//! use adk_server::a2a::interceptor::{
//!     A2aDelegationContext, A2aError, A2aInterceptor, InterceptorChain, InterceptorDecision,
//! };
//! use async_trait::async_trait;
//!
//! struct LoggingInterceptor;
//!
//! #[async_trait]
//! impl A2aInterceptor for LoggingInterceptor {
//!     async fn before_delegation(
//!         &self,
//!         ctx: &mut A2aDelegationContext,
//!     ) -> Result<InterceptorDecision, A2aError> {
//!         println!("Before: method={}", ctx.method);
//!         Ok(InterceptorDecision::Continue)
//!     }
//!
//!     async fn after_delegation(
//!         &self,
//!         ctx: &A2aDelegationContext,
//!         response: &mut serde_json::Value,
//!     ) -> Result<(), A2aError> {
//!         println!("After: method={}", ctx.method);
//!         Ok(())
//!     }
//! }
//!
//! # tokio_test::block_on(async {
//! let chain = InterceptorChain::new().add(LoggingInterceptor);
//!
//! let mut ctx = A2aDelegationContext {
//!     method: "tasks/send".to_string(),
//!     params: serde_json::json!({}),
//!     caller_id: None,
//!     metadata: std::collections::HashMap::new(),
//! };
//!
//! let decision = chain.run_before(&mut ctx).await.unwrap();
//! assert!(matches!(decision, InterceptorDecision::Continue));
//! # });
//! ```

use std::collections::HashMap;
use std::fmt;

use async_trait::async_trait;

/// Error type for A2A interceptor operations.
///
/// Represents failures that can occur during interceptor execution,
/// including validation errors, authentication failures, and internal errors.
///
/// # Example
///
/// ```rust
/// use adk_server::a2a::interceptor::A2aError;
///
/// let err = A2aError::new("authentication failed: invalid token");
/// assert_eq!(err.to_string(), "A2A interceptor error: authentication failed: invalid token");
///
/// let rejected = A2aError::rejected(-32001, "rate limit exceeded");
/// assert_eq!(rejected.code(), Some(-32001));
/// ```
#[derive(Debug, Clone)]
pub struct A2aError {
    message: String,
    code: Option<i32>,
}

impl A2aError {
    /// Creates a new `A2aError` with the given message.
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), code: None }
    }

    /// Creates a new `A2aError` representing a JSON-RPC rejection with a code and message.
    pub fn rejected(code: i32, message: impl Into<String>) -> Self {
        Self { message: message.into(), code: Some(code) }
    }

    /// Returns the optional JSON-RPC error code associated with this error.
    pub fn code(&self) -> Option<i32> {
        self.code
    }
}

impl fmt::Display for A2aError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(code) = self.code {
            write!(f, "A2A interceptor error (code {code}): {}", self.message)
        } else {
            write!(f, "A2A interceptor error: {}", self.message)
        }
    }
}

impl std::error::Error for A2aError {}

impl From<A2aError> for adk_core::AdkError {
    fn from(err: A2aError) -> Self {
        adk_core::AdkError::agent(err.to_string())
    }
}

/// Context available to interceptors during A2A request processing.
///
/// Contains the JSON-RPC method, parameters, optional caller identity,
/// and arbitrary metadata that interceptors can read or modify.
///
/// # Example
///
/// ```rust
/// use adk_server::a2a::interceptor::A2aDelegationContext;
/// use std::collections::HashMap;
///
/// let ctx = A2aDelegationContext {
///     method: "tasks/send".to_string(),
///     params: serde_json::json!({"message": "hello"}),
///     caller_id: Some("agent-123".to_string()),
///     metadata: HashMap::from([("tenant".to_string(), "acme".to_string())]),
/// };
///
/// assert_eq!(ctx.method, "tasks/send");
/// assert_eq!(ctx.caller_id.as_deref(), Some("agent-123"));
/// ```
#[derive(Debug, Clone)]
pub struct A2aDelegationContext {
    /// The JSON-RPC method being invoked (e.g., `"tasks/send"`).
    pub method: String,
    /// The JSON-RPC parameters for the request.
    pub params: serde_json::Value,
    /// Optional identifier of the calling agent or client.
    pub caller_id: Option<String>,
    /// Arbitrary key-value metadata that interceptors can read or modify.
    pub metadata: HashMap<String, String>,
}

/// Decision returned by an interceptor's `before_delegation` method.
///
/// Controls whether the request continues through the chain, is short-circuited
/// with a pre-built response, or is rejected with a JSON-RPC error.
///
/// # Example
///
/// ```rust
/// use adk_server::a2a::interceptor::InterceptorDecision;
///
/// // Allow the request to proceed
/// let decision = InterceptorDecision::Continue;
///
/// // Short-circuit with a cached response
/// let decision = InterceptorDecision::ShortCircuit(serde_json::json!({"cached": true}));
///
/// // Reject with a JSON-RPC error
/// let decision = InterceptorDecision::Reject { code: -32001, message: "unauthorized".to_string() };
/// ```
#[derive(Debug, Clone)]
pub enum InterceptorDecision {
    /// Pass the request to the next interceptor or to the executor.
    Continue,
    /// Return the given response immediately without calling subsequent
    /// interceptors or the executor.
    ShortCircuit(serde_json::Value),
    /// Return a JSON-RPC error response with the given code and message.
    Reject {
        /// JSON-RPC error code.
        code: i32,
        /// Human-readable error message.
        message: String,
    },
}

/// Trait for A2A request/response interceptors.
///
/// Interceptors can inspect, modify, or reject requests before they reach the
/// executor, and can inspect or modify responses after the executor completes.
///
/// # Execution Order
///
/// - `before_delegation` runs in registration order (first added → first called).
/// - `after_delegation` runs in reverse registration order (last added → first called).
///
/// # Example
///
/// ```rust
/// use adk_server::a2a::interceptor::{
///     A2aDelegationContext, A2aError, A2aInterceptor, InterceptorDecision,
/// };
/// use async_trait::async_trait;
///
/// struct AuthInterceptor {
///     valid_token: String,
/// }
///
/// #[async_trait]
/// impl A2aInterceptor for AuthInterceptor {
///     async fn before_delegation(
///         &self,
///         ctx: &mut A2aDelegationContext,
///     ) -> Result<InterceptorDecision, A2aError> {
///         match ctx.metadata.get("authorization") {
///             Some(token) if token == &self.valid_token => Ok(InterceptorDecision::Continue),
///             _ => Ok(InterceptorDecision::Reject {
///                 code: -32001,
///                 message: "unauthorized".to_string(),
///             }),
///         }
///     }
///
///     async fn after_delegation(
///         &self,
///         _ctx: &A2aDelegationContext,
///         _response: &mut serde_json::Value,
///     ) -> Result<(), A2aError> {
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait A2aInterceptor: Send + Sync {
    /// Inspect or modify the request before delegation.
    ///
    /// Return [`InterceptorDecision::Continue`] to pass to the next interceptor,
    /// [`InterceptorDecision::ShortCircuit`] to return a response immediately, or
    /// [`InterceptorDecision::Reject`] to return a JSON-RPC error.
    async fn before_delegation(
        &self,
        ctx: &mut A2aDelegationContext,
    ) -> Result<InterceptorDecision, A2aError>;

    /// Inspect or modify the response after delegation.
    ///
    /// Called in reverse registration order. Errors propagate immediately
    /// without calling remaining interceptors.
    async fn after_delegation(
        &self,
        ctx: &A2aDelegationContext,
        response: &mut serde_json::Value,
    ) -> Result<(), A2aError>;
}

/// An ordered chain of [`A2aInterceptor`] instances.
///
/// Executes `before_delegation` in registration order (first added runs first)
/// and `after_delegation` in reverse order (last added runs first), following
/// the standard middleware onion model.
///
/// # Example
///
/// ```rust
/// use adk_server::a2a::interceptor::{
///     A2aDelegationContext, A2aError, A2aInterceptor, InterceptorChain, InterceptorDecision,
/// };
/// use async_trait::async_trait;
///
/// struct PassthroughInterceptor;
///
/// #[async_trait]
/// impl A2aInterceptor for PassthroughInterceptor {
///     async fn before_delegation(
///         &self,
///         _ctx: &mut A2aDelegationContext,
///     ) -> Result<InterceptorDecision, A2aError> {
///         Ok(InterceptorDecision::Continue)
///     }
///
///     async fn after_delegation(
///         &self,
///         _ctx: &A2aDelegationContext,
///         _response: &mut serde_json::Value,
///     ) -> Result<(), A2aError> {
///         Ok(())
///     }
/// }
///
/// # tokio_test::block_on(async {
/// let chain = InterceptorChain::new()
///     .add(PassthroughInterceptor)
///     .add(PassthroughInterceptor);
///
/// let mut ctx = A2aDelegationContext {
///     method: "tasks/send".to_string(),
///     params: serde_json::json!({}),
///     caller_id: None,
///     metadata: std::collections::HashMap::new(),
/// };
///
/// let decision = chain.run_before(&mut ctx).await.unwrap();
/// assert!(matches!(decision, InterceptorDecision::Continue));
///
/// let mut response = serde_json::json!({"result": "ok"});
/// chain.run_after(&ctx, &mut response).await.unwrap();
/// # });
/// ```
pub struct InterceptorChain {
    interceptors: Vec<Box<dyn A2aInterceptor>>,
}

impl InterceptorChain {
    /// Creates a new empty interceptor chain.
    pub fn new() -> Self {
        Self { interceptors: Vec::new() }
    }

    /// Adds an interceptor to the end of the chain.
    ///
    /// Interceptors are executed in the order they are added for `before_delegation`,
    /// and in reverse order for `after_delegation`.
    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, interceptor: impl A2aInterceptor + 'static) -> Self {
        self.interceptors.push(Box::new(interceptor));
        self
    }

    /// Returns the number of interceptors in the chain.
    pub fn len(&self) -> usize {
        self.interceptors.len()
    }

    /// Returns `true` if the chain contains no interceptors.
    pub fn is_empty(&self) -> bool {
        self.interceptors.is_empty()
    }

    /// Runs all interceptors' `before_delegation` in registration order.
    ///
    /// Stops early if any interceptor returns [`InterceptorDecision::ShortCircuit`]
    /// or [`InterceptorDecision::Reject`].
    ///
    /// # Errors
    ///
    /// Returns [`A2aError`] if any interceptor fails.
    pub async fn run_before(
        &self,
        ctx: &mut A2aDelegationContext,
    ) -> Result<InterceptorDecision, A2aError> {
        for interceptor in &self.interceptors {
            match interceptor.before_delegation(ctx).await? {
                InterceptorDecision::Continue => continue,
                decision => return Ok(decision),
            }
        }
        Ok(InterceptorDecision::Continue)
    }

    /// Runs all interceptors' `after_delegation` in reverse registration order.
    ///
    /// Stops early if any interceptor returns an error.
    ///
    /// # Errors
    ///
    /// Returns [`A2aError`] if any interceptor fails.
    pub async fn run_after(
        &self,
        ctx: &A2aDelegationContext,
        response: &mut serde_json::Value,
    ) -> Result<(), A2aError> {
        for interceptor in self.interceptors.iter().rev() {
            interceptor.after_delegation(ctx, response).await?;
        }
        Ok(())
    }
}

impl Default for InterceptorChain {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for InterceptorChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InterceptorChain")
            .field("interceptor_count", &self.interceptors.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CountingInterceptor {
        id: usize,
        order: std::sync::Arc<std::sync::Mutex<Vec<(usize, &'static str)>>>,
    }

    #[async_trait]
    impl A2aInterceptor for CountingInterceptor {
        async fn before_delegation(
            &self,
            _ctx: &mut A2aDelegationContext,
        ) -> Result<InterceptorDecision, A2aError> {
            self.order.lock().unwrap().push((self.id, "before"));
            Ok(InterceptorDecision::Continue)
        }

        async fn after_delegation(
            &self,
            _ctx: &A2aDelegationContext,
            _response: &mut serde_json::Value,
        ) -> Result<(), A2aError> {
            self.order.lock().unwrap().push((self.id, "after"));
            Ok(())
        }
    }

    struct RejectingInterceptor;

    #[async_trait]
    impl A2aInterceptor for RejectingInterceptor {
        async fn before_delegation(
            &self,
            _ctx: &mut A2aDelegationContext,
        ) -> Result<InterceptorDecision, A2aError> {
            Ok(InterceptorDecision::Reject { code: -32001, message: "denied".to_string() })
        }

        async fn after_delegation(
            &self,
            _ctx: &A2aDelegationContext,
            _response: &mut serde_json::Value,
        ) -> Result<(), A2aError> {
            Ok(())
        }
    }

    struct ShortCircuitInterceptor;

    #[async_trait]
    impl A2aInterceptor for ShortCircuitInterceptor {
        async fn before_delegation(
            &self,
            _ctx: &mut A2aDelegationContext,
        ) -> Result<InterceptorDecision, A2aError> {
            Ok(InterceptorDecision::ShortCircuit(serde_json::json!({"cached": true})))
        }

        async fn after_delegation(
            &self,
            _ctx: &A2aDelegationContext,
            _response: &mut serde_json::Value,
        ) -> Result<(), A2aError> {
            Ok(())
        }
    }

    fn make_ctx() -> A2aDelegationContext {
        A2aDelegationContext {
            method: "tasks/send".to_string(),
            params: serde_json::json!({}),
            caller_id: None,
            metadata: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_empty_chain_returns_continue() {
        let chain = InterceptorChain::new();
        let mut ctx = make_ctx();
        let decision = chain.run_before(&mut ctx).await.unwrap();
        assert!(matches!(decision, InterceptorDecision::Continue));
    }

    #[tokio::test]
    async fn test_before_executes_in_registration_order() {
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let chain = InterceptorChain::new()
            .add(CountingInterceptor { id: 1, order: order.clone() })
            .add(CountingInterceptor { id: 2, order: order.clone() })
            .add(CountingInterceptor { id: 3, order: order.clone() });

        let mut ctx = make_ctx();
        chain.run_before(&mut ctx).await.unwrap();

        let recorded = order.lock().unwrap();
        assert_eq!(recorded.as_slice(), &[(1, "before"), (2, "before"), (3, "before")]);
    }

    #[tokio::test]
    async fn test_after_executes_in_reverse_order() {
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let chain = InterceptorChain::new()
            .add(CountingInterceptor { id: 1, order: order.clone() })
            .add(CountingInterceptor { id: 2, order: order.clone() })
            .add(CountingInterceptor { id: 3, order: order.clone() });

        let ctx = make_ctx();
        let mut response = serde_json::json!({"result": "ok"});
        chain.run_after(&ctx, &mut response).await.unwrap();

        let recorded = order.lock().unwrap();
        assert_eq!(recorded.as_slice(), &[(3, "after"), (2, "after"), (1, "after")]);
    }

    #[tokio::test]
    async fn test_reject_stops_chain() {
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let chain = InterceptorChain::new()
            .add(CountingInterceptor { id: 1, order: order.clone() })
            .add(RejectingInterceptor)
            .add(CountingInterceptor { id: 3, order: order.clone() });

        let mut ctx = make_ctx();
        let decision = chain.run_before(&mut ctx).await.unwrap();

        assert!(matches!(decision, InterceptorDecision::Reject { code: -32001, .. }));
        let recorded = order.lock().unwrap();
        // Only interceptor 1 ran before the rejection
        assert_eq!(recorded.as_slice(), &[(1, "before")]);
    }

    #[tokio::test]
    async fn test_short_circuit_stops_chain() {
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let chain = InterceptorChain::new()
            .add(CountingInterceptor { id: 1, order: order.clone() })
            .add(ShortCircuitInterceptor)
            .add(CountingInterceptor { id: 3, order: order.clone() });

        let mut ctx = make_ctx();
        let decision = chain.run_before(&mut ctx).await.unwrap();

        match decision {
            InterceptorDecision::ShortCircuit(val) => {
                assert_eq!(val, serde_json::json!({"cached": true}));
            }
            _ => panic!("expected ShortCircuit"),
        }
        let recorded = order.lock().unwrap();
        assert_eq!(recorded.as_slice(), &[(1, "before")]);
    }

    #[tokio::test]
    async fn test_chain_len_and_is_empty() {
        let chain = InterceptorChain::new();
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);

        let chain = chain.add(ShortCircuitInterceptor);
        assert!(!chain.is_empty());
        assert_eq!(chain.len(), 1);
    }

    #[tokio::test]
    async fn test_context_mutation_propagates() {
        struct MutatingInterceptor;

        #[async_trait]
        impl A2aInterceptor for MutatingInterceptor {
            async fn before_delegation(
                &self,
                ctx: &mut A2aDelegationContext,
            ) -> Result<InterceptorDecision, A2aError> {
                ctx.metadata.insert("enriched".to_string(), "true".to_string());
                Ok(InterceptorDecision::Continue)
            }

            async fn after_delegation(
                &self,
                _ctx: &A2aDelegationContext,
                _response: &mut serde_json::Value,
            ) -> Result<(), A2aError> {
                Ok(())
            }
        }

        let chain = InterceptorChain::new().add(MutatingInterceptor);
        let mut ctx = make_ctx();
        chain.run_before(&mut ctx).await.unwrap();

        assert_eq!(ctx.metadata.get("enriched"), Some(&"true".to_string()));
    }

    #[tokio::test]
    async fn test_a2a_error_display() {
        let err = A2aError::new("something failed");
        assert_eq!(err.to_string(), "A2A interceptor error: something failed");

        let err = A2aError::rejected(-32001, "rate limited");
        assert_eq!(err.to_string(), "A2A interceptor error (code -32001): rate limited");
        assert_eq!(err.code(), Some(-32001));
    }
}
