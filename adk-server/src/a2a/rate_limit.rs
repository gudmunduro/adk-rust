//! Token bucket rate limiting interceptor for A2A requests.
//!
//! This module provides a [`RateLimitInterceptor`] that enforces per-client request
//! rate limits using a token bucket algorithm. Each client (identified by `caller_id`)
//! gets its own bucket, and requests without a `caller_id` share a global bucket.
//!
//! # Algorithm
//!
//! The token bucket algorithm works as follows:
//! - Each bucket holds up to `burst` tokens.
//! - Tokens are consumed one per request.
//! - Tokens refill at a rate of `rps` (requests per second) based on elapsed time.
//! - If no tokens are available, the request is rejected.
//!
//! # Example
//!
//! ```rust
//! use adk_server::a2a::interceptor::{
//!     A2aDelegationContext, A2aInterceptor, InterceptorChain, InterceptorDecision,
//! };
//! use adk_server::a2a::rate_limit::RateLimitInterceptor;
//! use std::collections::HashMap;
//!
//! # tokio_test::block_on(async {
//! // Allow 10 requests per second with a burst of 20
//! let limiter = RateLimitInterceptor::new(10, 20);
//! let chain = InterceptorChain::new().add(limiter);
//!
//! let mut ctx = A2aDelegationContext {
//!     method: "tasks/send".to_string(),
//!     params: serde_json::json!({}),
//!     caller_id: Some("client-1".to_string()),
//!     metadata: HashMap::new(),
//! };
//!
//! // First request should succeed (bucket starts full)
//! let decision = chain.run_before(&mut ctx).await.unwrap();
//! assert!(matches!(decision, InterceptorDecision::Continue));
//! # });
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::interceptor::{A2aDelegationContext, A2aError, A2aInterceptor, InterceptorDecision};

/// A single token bucket tracking available tokens and last refill time.
#[derive(Debug, Clone)]
struct TokenBucket {
    /// Current number of available tokens (can be fractional during refill).
    tokens: f64,
    /// Timestamp of the last token refill calculation.
    last_refill: Instant,
}

impl TokenBucket {
    /// Creates a new bucket filled to the given capacity.
    fn new(capacity: u32) -> Self {
        Self { tokens: f64::from(capacity), last_refill: Instant::now() }
    }

    /// Refills tokens based on elapsed time and attempts to consume one token.
    ///
    /// Returns `true` if a token was successfully consumed, `false` if the bucket
    /// is empty after refill.
    fn try_consume(&mut self, rps: u32, burst: u32) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.last_refill = now;

        // Refill tokens based on elapsed time, capped at burst
        self.tokens = (self.tokens + elapsed * f64::from(rps)).min(f64::from(burst));

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// A2A interceptor that enforces per-client request rate limits using a token bucket algorithm.
///
/// Each client identified by `caller_id` gets its own token bucket. Requests without
/// a `caller_id` share a single global bucket keyed by `"__global__"`.
///
/// # Configuration
///
/// - `rps` — The token refill rate in requests per second.
/// - `burst` — The maximum number of tokens in the bucket, allowing short bursts
///   above the sustained rate.
///
/// # Rejection
///
/// When a client exceeds the rate limit, the interceptor rejects the request with
/// JSON-RPC error code `-32002` and message `"rate limit exceeded"`.
///
/// # Example
///
/// ```rust
/// use adk_server::a2a::interceptor::{
///     A2aDelegationContext, A2aInterceptor, InterceptorDecision,
/// };
/// use adk_server::a2a::rate_limit::RateLimitInterceptor;
/// use std::collections::HashMap;
///
/// # tokio_test::block_on(async {
/// let limiter = RateLimitInterceptor::new(5, 5);
///
/// let mut ctx = A2aDelegationContext {
///     method: "tasks/send".to_string(),
///     params: serde_json::json!({}),
///     caller_id: Some("agent-a".to_string()),
///     metadata: HashMap::new(),
/// };
///
/// // First 5 requests succeed (burst capacity)
/// for _ in 0..5 {
///     let decision = limiter.before_delegation(&mut ctx).await.unwrap();
///     assert!(matches!(decision, InterceptorDecision::Continue));
/// }
///
/// // 6th request is rejected (bucket exhausted)
/// let decision = limiter.before_delegation(&mut ctx).await.unwrap();
/// assert!(matches!(decision, InterceptorDecision::Reject { code: -32002, .. }));
/// # });
/// ```
#[derive(Debug, Clone)]
pub struct RateLimitInterceptor {
    /// Token refill rate in requests per second.
    pub rps: u32,
    /// Maximum tokens in the bucket (burst capacity).
    pub burst: u32,
    /// Per-client token buckets.
    buckets: Arc<Mutex<HashMap<String, TokenBucket>>>,
}

impl RateLimitInterceptor {
    /// Creates a new `RateLimitInterceptor` with the given rate and burst capacity.
    ///
    /// # Arguments
    ///
    /// * `rps` - The sustained request rate in requests per second (token refill rate).
    /// * `burst` - The maximum number of tokens in the bucket, allowing short bursts.
    ///
    /// # Example
    ///
    /// ```rust
    /// use adk_server::a2a::rate_limit::RateLimitInterceptor;
    ///
    /// // 100 requests/second sustained, burst up to 200
    /// let limiter = RateLimitInterceptor::new(100, 200);
    /// ```
    pub fn new(rps: u32, burst: u32) -> Self {
        Self { rps, burst, buckets: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// Returns the key used to identify the client's bucket.
    ///
    /// Uses `caller_id` if present, otherwise falls back to a global bucket key.
    fn bucket_key(ctx: &A2aDelegationContext) -> String {
        ctx.caller_id.clone().unwrap_or_else(|| "__global__".to_string())
    }
}

#[async_trait]
impl A2aInterceptor for RateLimitInterceptor {
    /// Checks if the client has available tokens. Consumes one token on success,
    /// rejects with code `-32002` if the bucket is empty.
    async fn before_delegation(
        &self,
        ctx: &mut A2aDelegationContext,
    ) -> Result<InterceptorDecision, A2aError> {
        let key = Self::bucket_key(ctx);
        let mut buckets = self.buckets.lock().await;

        let bucket = buckets.entry(key).or_insert_with(|| TokenBucket::new(self.burst));

        if bucket.try_consume(self.rps, self.burst) {
            Ok(InterceptorDecision::Continue)
        } else {
            Ok(InterceptorDecision::Reject {
                code: -32002,
                message: "rate limit exceeded".to_string(),
            })
        }
    }

    /// No-op for the rate limit interceptor. Rate limiting is handled entirely
    /// in `before_delegation`.
    async fn after_delegation(
        &self,
        _ctx: &A2aDelegationContext,
        _response: &mut serde_json::Value,
    ) -> Result<(), A2aError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_ctx(caller_id: Option<&str>) -> A2aDelegationContext {
        A2aDelegationContext {
            method: "tasks/send".to_string(),
            params: serde_json::json!({}),
            caller_id: caller_id.map(String::from),
            metadata: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_first_request_allowed() {
        let limiter = RateLimitInterceptor::new(10, 10);
        let mut ctx = make_ctx(Some("client-1"));

        let decision = limiter.before_delegation(&mut ctx).await.unwrap();
        assert!(matches!(decision, InterceptorDecision::Continue));
    }

    #[tokio::test]
    async fn test_burst_capacity_allows_multiple_requests() {
        let limiter = RateLimitInterceptor::new(1, 5);
        let mut ctx = make_ctx(Some("client-1"));

        for _ in 0..5 {
            let decision = limiter.before_delegation(&mut ctx).await.unwrap();
            assert!(matches!(decision, InterceptorDecision::Continue));
        }
    }

    #[tokio::test]
    async fn test_exceeding_burst_rejects() {
        let limiter = RateLimitInterceptor::new(1, 3);
        let mut ctx = make_ctx(Some("client-1"));

        // Exhaust the burst
        for _ in 0..3 {
            let decision = limiter.before_delegation(&mut ctx).await.unwrap();
            assert!(matches!(decision, InterceptorDecision::Continue));
        }

        // Next request should be rejected
        let decision = limiter.before_delegation(&mut ctx).await.unwrap();
        match decision {
            InterceptorDecision::Reject { code, message } => {
                assert_eq!(code, -32002);
                assert_eq!(message, "rate limit exceeded");
            }
            _ => panic!("expected Reject"),
        }
    }

    #[tokio::test]
    async fn test_per_client_isolation() {
        let limiter = RateLimitInterceptor::new(1, 2);

        // Exhaust client-1's bucket
        let mut ctx1 = make_ctx(Some("client-1"));
        for _ in 0..2 {
            limiter.before_delegation(&mut ctx1).await.unwrap();
        }
        let decision = limiter.before_delegation(&mut ctx1).await.unwrap();
        assert!(matches!(decision, InterceptorDecision::Reject { .. }));

        // client-2 should still have tokens
        let mut ctx2 = make_ctx(Some("client-2"));
        let decision = limiter.before_delegation(&mut ctx2).await.unwrap();
        assert!(matches!(decision, InterceptorDecision::Continue));
    }

    #[tokio::test]
    async fn test_no_caller_id_uses_global_bucket() {
        let limiter = RateLimitInterceptor::new(1, 2);
        let mut ctx = make_ctx(None);

        // Exhaust global bucket
        for _ in 0..2 {
            let decision = limiter.before_delegation(&mut ctx).await.unwrap();
            assert!(matches!(decision, InterceptorDecision::Continue));
        }

        let decision = limiter.before_delegation(&mut ctx).await.unwrap();
        assert!(matches!(decision, InterceptorDecision::Reject { .. }));
    }

    #[tokio::test]
    async fn test_tokens_refill_over_time() {
        let limiter = RateLimitInterceptor::new(100, 1);
        let mut ctx = make_ctx(Some("client-1"));

        // Consume the single token
        let decision = limiter.before_delegation(&mut ctx).await.unwrap();
        assert!(matches!(decision, InterceptorDecision::Continue));

        // Immediately should be rejected
        let decision = limiter.before_delegation(&mut ctx).await.unwrap();
        assert!(matches!(decision, InterceptorDecision::Reject { .. }));

        // Wait enough time for a refill (100 rps = 10ms per token)
        tokio::time::sleep(tokio::time::Duration::from_millis(15)).await;

        // Should be allowed again
        let decision = limiter.before_delegation(&mut ctx).await.unwrap();
        assert!(matches!(decision, InterceptorDecision::Continue));
    }

    #[tokio::test]
    async fn test_after_delegation_is_noop() {
        let limiter = RateLimitInterceptor::new(10, 10);
        let ctx = A2aDelegationContext {
            method: "tasks/send".to_string(),
            params: serde_json::json!({}),
            caller_id: Some("client-1".to_string()),
            metadata: HashMap::new(),
        };
        let mut response = serde_json::json!({"result": "ok"});

        let result = limiter.after_delegation(&ctx, &mut response).await;
        assert!(result.is_ok());
        assert_eq!(response, serde_json::json!({"result": "ok"}));
    }

    #[tokio::test]
    async fn test_rejection_code_is_minus_32002() {
        let limiter = RateLimitInterceptor::new(1, 0);
        let mut ctx = make_ctx(Some("client-1"));

        // With burst=0, bucket starts empty so first request is rejected
        // Actually with burst=0, tokens start at 0.0 so it should reject immediately
        let decision = limiter.before_delegation(&mut ctx).await.unwrap();
        match decision {
            InterceptorDecision::Reject { code, message } => {
                assert_eq!(code, -32002);
                assert_eq!(message, "rate limit exceeded");
            }
            _ => panic!("expected Reject with burst=0"),
        }
    }
}
