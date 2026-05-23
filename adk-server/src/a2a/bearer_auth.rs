//! Bearer token authentication interceptor for A2A requests.
//!
//! This module provides a [`BearerAuthInterceptor`] that validates bearer tokens
//! from the `Authorization` header (stored in `ctx.metadata["authorization"]`).
//! It delegates token validation to a user-provided [`TokenValidator`] implementation.
//!
//! # Architecture
//!
//! The interceptor extracts the bearer token from the `authorization` metadata entry,
//! strips the `"Bearer "` prefix, and passes the raw token to the validator. On success,
//! the validator returns an optional `caller_id` that is set on the delegation context.
//! On failure, the request is rejected with a JSON-RPC error.
//!
//! # Example
//!
//! ```rust
//! use adk_server::a2a::interceptor::{
//!     A2aDelegationContext, A2aError, A2aInterceptor, InterceptorChain, InterceptorDecision,
//! };
//! use adk_server::a2a::bearer_auth::{BearerAuthInterceptor, TokenValidator};
//! use async_trait::async_trait;
//! use std::sync::Arc;
//!
//! struct MyValidator;
//!
//! #[async_trait]
//! impl TokenValidator for MyValidator {
//!     async fn validate_token(&self, token: &str) -> Result<Option<String>, A2aError> {
//!         if token == "valid-token-123" {
//!             Ok(Some("user-42".to_string()))
//!         } else {
//!             Err(A2aError::rejected(-32001, "invalid token"))
//!         }
//!     }
//! }
//!
//! # tokio_test::block_on(async {
//! let interceptor = BearerAuthInterceptor::new(Arc::new(MyValidator));
//! let chain = InterceptorChain::new().add(interceptor);
//!
//! let mut ctx = A2aDelegationContext {
//!     method: "tasks/send".to_string(),
//!     params: serde_json::json!({}),
//!     caller_id: None,
//!     metadata: std::collections::HashMap::from([
//!         ("authorization".to_string(), "Bearer valid-token-123".to_string()),
//!     ]),
//! };
//!
//! let decision = chain.run_before(&mut ctx).await.unwrap();
//! assert!(matches!(decision, InterceptorDecision::Continue));
//! assert_eq!(ctx.caller_id.as_deref(), Some("user-42"));
//! # });
//! ```

use std::sync::Arc;

use async_trait::async_trait;

use super::interceptor::{A2aDelegationContext, A2aError, A2aInterceptor, InterceptorDecision};

/// Trait for validating bearer tokens extracted from the Authorization header.
///
/// Implementations perform the actual token verification (e.g., JWT signature check,
/// database lookup, or external auth service call) and return an optional caller
/// identity on success.
///
/// # Example
///
/// ```rust
/// use adk_server::a2a::bearer_auth::TokenValidator;
/// use adk_server::a2a::interceptor::A2aError;
/// use async_trait::async_trait;
///
/// struct StaticValidator {
///     expected: String,
/// }
///
/// #[async_trait]
/// impl TokenValidator for StaticValidator {
///     async fn validate_token(&self, token: &str) -> Result<Option<String>, A2aError> {
///         if token == self.expected {
///             Ok(Some("authenticated-user".to_string()))
///         } else {
///             Err(A2aError::rejected(-32001, "invalid bearer token"))
///         }
///     }
/// }
/// ```
#[async_trait]
pub trait TokenValidator: Send + Sync {
    /// Validates the given bearer token.
    ///
    /// # Arguments
    ///
    /// * `token` - The raw bearer token (without the `"Bearer "` prefix).
    ///
    /// # Returns
    ///
    /// * `Ok(Some(caller_id))` — Token is valid; the returned string identifies the caller.
    /// * `Ok(None)` — Token is valid but no caller identity is available.
    /// * `Err(A2aError)` — Token is invalid or validation failed.
    async fn validate_token(&self, token: &str) -> Result<Option<String>, A2aError>;
}

/// A2A interceptor that validates bearer tokens in the Authorization header.
///
/// Extracts the bearer token from `ctx.metadata["authorization"]`, validates it
/// using the provided [`TokenValidator`], and sets `ctx.caller_id` on success.
/// Rejects the request if no token is present or if validation fails.
///
/// # Example
///
/// ```rust
/// use adk_server::a2a::bearer_auth::{BearerAuthInterceptor, TokenValidator};
/// use adk_server::a2a::interceptor::{
///     A2aDelegationContext, A2aError, A2aInterceptor, InterceptorChain, InterceptorDecision,
/// };
/// use async_trait::async_trait;
/// use std::sync::Arc;
///
/// struct AlwaysValid;
///
/// #[async_trait]
/// impl TokenValidator for AlwaysValid {
///     async fn validate_token(&self, _token: &str) -> Result<Option<String>, A2aError> {
///         Ok(Some("anonymous".to_string()))
///     }
/// }
///
/// # tokio_test::block_on(async {
/// let interceptor = BearerAuthInterceptor::new(Arc::new(AlwaysValid));
///
/// let mut ctx = A2aDelegationContext {
///     method: "tasks/send".to_string(),
///     params: serde_json::json!({}),
///     caller_id: None,
///     metadata: std::collections::HashMap::from([
///         ("authorization".to_string(), "Bearer my-token".to_string()),
///     ]),
/// };
///
/// let decision = interceptor.before_delegation(&mut ctx).await.unwrap();
/// assert!(matches!(decision, InterceptorDecision::Continue));
/// assert_eq!(ctx.caller_id.as_deref(), Some("anonymous"));
/// # });
/// ```
#[derive(Clone)]
pub struct BearerAuthInterceptor {
    /// The token validator used to verify bearer tokens.
    pub validator: Arc<dyn TokenValidator>,
}

impl BearerAuthInterceptor {
    /// Creates a new `BearerAuthInterceptor` with the given token validator.
    ///
    /// # Arguments
    ///
    /// * `validator` - An implementation of [`TokenValidator`] wrapped in an `Arc`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use adk_server::a2a::bearer_auth::{BearerAuthInterceptor, TokenValidator};
    /// use adk_server::a2a::interceptor::A2aError;
    /// use async_trait::async_trait;
    /// use std::sync::Arc;
    ///
    /// struct MyValidator;
    ///
    /// #[async_trait]
    /// impl TokenValidator for MyValidator {
    ///     async fn validate_token(&self, _token: &str) -> Result<Option<String>, A2aError> {
    ///         Ok(None)
    ///     }
    /// }
    ///
    /// let interceptor = BearerAuthInterceptor::new(Arc::new(MyValidator));
    /// ```
    pub fn new(validator: Arc<dyn TokenValidator>) -> Self {
        Self { validator }
    }

    /// Extracts the bearer token from the authorization header value.
    ///
    /// Returns `None` if the value does not start with `"Bearer "` (case-insensitive prefix).
    fn extract_bearer_token(auth_value: &str) -> Option<&str> {
        let trimmed = auth_value.trim();
        if trimmed.len() > 7 && trimmed[..7].eq_ignore_ascii_case("bearer ") {
            Some(&trimmed[7..])
        } else {
            None
        }
    }
}

#[async_trait]
impl A2aInterceptor for BearerAuthInterceptor {
    /// Validates the bearer token from `ctx.metadata["authorization"]`.
    ///
    /// On success, sets `ctx.caller_id` to the identity returned by the validator.
    /// On failure (missing header, malformed token, or validation error), rejects
    /// the request with a JSON-RPC error code `-32001`.
    async fn before_delegation(
        &self,
        ctx: &mut A2aDelegationContext,
    ) -> Result<InterceptorDecision, A2aError> {
        let auth_header = match ctx.metadata.get("authorization") {
            Some(value) => value.clone(),
            None => {
                return Ok(InterceptorDecision::Reject {
                    code: -32001,
                    message: "missing authorization header".to_string(),
                });
            }
        };

        let token = match Self::extract_bearer_token(&auth_header) {
            Some(t) => t,
            None => {
                return Ok(InterceptorDecision::Reject {
                    code: -32001,
                    message: "invalid authorization header: expected Bearer scheme".to_string(),
                });
            }
        };

        match self.validator.validate_token(token).await {
            Ok(caller_id) => {
                ctx.caller_id = caller_id;
                Ok(InterceptorDecision::Continue)
            }
            Err(err) => Ok(InterceptorDecision::Reject {
                code: err.code().unwrap_or(-32001),
                message: err.to_string(),
            }),
        }
    }

    /// No-op for the bearer auth interceptor. Authentication is handled entirely
    /// in `before_delegation`.
    async fn after_delegation(
        &self,
        _ctx: &A2aDelegationContext,
        _response: &mut serde_json::Value,
    ) -> Result<(), A2aError> {
        Ok(())
    }
}

impl std::fmt::Debug for BearerAuthInterceptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BearerAuthInterceptor").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct AcceptAllValidator;

    #[async_trait]
    impl TokenValidator for AcceptAllValidator {
        async fn validate_token(&self, _token: &str) -> Result<Option<String>, A2aError> {
            Ok(Some("test-user".to_string()))
        }
    }

    struct RejectAllValidator;

    #[async_trait]
    impl TokenValidator for RejectAllValidator {
        async fn validate_token(&self, _token: &str) -> Result<Option<String>, A2aError> {
            Err(A2aError::rejected(-32001, "token rejected"))
        }
    }

    struct NoCaller;

    #[async_trait]
    impl TokenValidator for NoCaller {
        async fn validate_token(&self, _token: &str) -> Result<Option<String>, A2aError> {
            Ok(None)
        }
    }

    fn make_ctx_with_auth(auth: &str) -> A2aDelegationContext {
        A2aDelegationContext {
            method: "tasks/send".to_string(),
            params: serde_json::json!({}),
            caller_id: None,
            metadata: HashMap::from([("authorization".to_string(), auth.to_string())]),
        }
    }

    fn make_ctx_no_auth() -> A2aDelegationContext {
        A2aDelegationContext {
            method: "tasks/send".to_string(),
            params: serde_json::json!({}),
            caller_id: None,
            metadata: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_valid_bearer_token_sets_caller_id() {
        let interceptor = BearerAuthInterceptor::new(Arc::new(AcceptAllValidator));
        let mut ctx = make_ctx_with_auth("Bearer my-secret-token");

        let decision = interceptor.before_delegation(&mut ctx).await.unwrap();
        assert!(matches!(decision, InterceptorDecision::Continue));
        assert_eq!(ctx.caller_id.as_deref(), Some("test-user"));
    }

    #[tokio::test]
    async fn test_valid_bearer_token_no_caller_id() {
        let interceptor = BearerAuthInterceptor::new(Arc::new(NoCaller));
        let mut ctx = make_ctx_with_auth("Bearer some-token");

        let decision = interceptor.before_delegation(&mut ctx).await.unwrap();
        assert!(matches!(decision, InterceptorDecision::Continue));
        assert_eq!(ctx.caller_id, None);
    }

    #[tokio::test]
    async fn test_missing_authorization_header_rejects() {
        let interceptor = BearerAuthInterceptor::new(Arc::new(AcceptAllValidator));
        let mut ctx = make_ctx_no_auth();

        let decision = interceptor.before_delegation(&mut ctx).await.unwrap();
        match decision {
            InterceptorDecision::Reject { code, message } => {
                assert_eq!(code, -32001);
                assert!(message.contains("missing authorization header"));
            }
            _ => panic!("expected Reject"),
        }
    }

    #[tokio::test]
    async fn test_non_bearer_scheme_rejects() {
        let interceptor = BearerAuthInterceptor::new(Arc::new(AcceptAllValidator));
        let mut ctx = make_ctx_with_auth("Basic dXNlcjpwYXNz");

        let decision = interceptor.before_delegation(&mut ctx).await.unwrap();
        match decision {
            InterceptorDecision::Reject { code, message } => {
                assert_eq!(code, -32001);
                assert!(message.contains("expected Bearer scheme"));
            }
            _ => panic!("expected Reject"),
        }
    }

    #[tokio::test]
    async fn test_invalid_token_rejects() {
        let interceptor = BearerAuthInterceptor::new(Arc::new(RejectAllValidator));
        let mut ctx = make_ctx_with_auth("Bearer bad-token");

        let decision = interceptor.before_delegation(&mut ctx).await.unwrap();
        match decision {
            InterceptorDecision::Reject { code, message } => {
                assert_eq!(code, -32001);
                assert!(message.contains("token rejected"));
            }
            _ => panic!("expected Reject"),
        }
    }

    #[tokio::test]
    async fn test_bearer_prefix_case_insensitive() {
        let interceptor = BearerAuthInterceptor::new(Arc::new(AcceptAllValidator));
        let mut ctx = make_ctx_with_auth("BEARER my-token");

        let decision = interceptor.before_delegation(&mut ctx).await.unwrap();
        assert!(matches!(decision, InterceptorDecision::Continue));
        assert_eq!(ctx.caller_id.as_deref(), Some("test-user"));
    }

    #[tokio::test]
    async fn test_bearer_prefix_with_leading_whitespace() {
        let interceptor = BearerAuthInterceptor::new(Arc::new(AcceptAllValidator));
        let mut ctx = make_ctx_with_auth("  Bearer my-token");

        let decision = interceptor.before_delegation(&mut ctx).await.unwrap();
        assert!(matches!(decision, InterceptorDecision::Continue));
        assert_eq!(ctx.caller_id.as_deref(), Some("test-user"));
    }

    #[tokio::test]
    async fn test_after_delegation_is_noop() {
        let interceptor = BearerAuthInterceptor::new(Arc::new(AcceptAllValidator));
        let ctx = A2aDelegationContext {
            method: "tasks/send".to_string(),
            params: serde_json::json!({}),
            caller_id: Some("user".to_string()),
            metadata: HashMap::new(),
        };
        let mut response = serde_json::json!({"result": "ok"});

        let result = interceptor.after_delegation(&ctx, &mut response).await;
        assert!(result.is_ok());
        assert_eq!(response, serde_json::json!({"result": "ok"}));
    }

    #[tokio::test]
    async fn test_empty_bearer_value_rejects() {
        let interceptor = BearerAuthInterceptor::new(Arc::new(AcceptAllValidator));
        let mut ctx = make_ctx_with_auth("Bearer");

        let decision = interceptor.before_delegation(&mut ctx).await.unwrap();
        match decision {
            InterceptorDecision::Reject { code, message } => {
                assert_eq!(code, -32001);
                assert!(message.contains("expected Bearer scheme"));
            }
            _ => panic!("expected Reject"),
        }
    }
}
