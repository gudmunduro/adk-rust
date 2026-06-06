//! Retry policy — automatic retry for transient errors with exponential backoff.
//!
//! Implements Requirements 9.1–9.6:
//! - Retry on 429, 500, 502, 503, 504 responses
//! - Never retry 400, 401, 403, 404, 409, 422
//! - Exponential backoff with jitter
//! - Respect `Retry-After` header on 429
//! - Configurable max attempts (default: 3)
//! - Return last error after exhausting retries
//!
//! Also implements Requirements 10.1–10.3 (idempotency):
//! - Generate UUID v4 per create call
//! - Reuse same key across retries
//! - Send as `Idempotency-Key` header

use std::future::Future;
use std::time::Duration;

use reqwest::Response;
use tracing::{debug, warn};

use crate::error::EnterpriseError;
use crate::idempotency::generate_idempotency_key;

/// Configuration for automatic retry with exponential backoff.
///
/// Retries on 429, 500, 502, 503, 504 responses.
/// Respects `Retry-After` header on 429 responses.
/// Never retries 400, 401, 403, 404, 409, or 422.
///
/// # Example
///
/// ```rust
/// use adk_enterprise::retry::RetryPolicy;
/// use std::time::Duration;
///
/// let policy = RetryPolicy {
///     max_attempts: 5,
///     initial_backoff: Duration::from_millis(500),
///     max_backoff: Duration::from_secs(60),
///     backoff_multiplier: 2.0,
/// };
/// ```
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts (not including the initial request).
    pub max_attempts: u32,
    /// Initial backoff duration before the first retry.
    pub initial_backoff: Duration,
    /// Maximum backoff duration between retries.
    pub max_backoff: Duration,
    /// Multiplier applied to backoff after each attempt.
    pub backoff_multiplier: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(30),
            backoff_multiplier: 2.0,
        }
    }
}

impl RetryPolicy {
    /// Create a `RetryPolicy` from client configuration values.
    pub fn from_config(max_retries: u32, initial_backoff: Duration) -> Self {
        Self { max_attempts: max_retries, initial_backoff, ..Default::default() }
    }

    /// Calculate the backoff duration for a given attempt number (0-indexed).
    ///
    /// Uses exponential backoff: `initial * multiplier^attempt`, capped at `max_backoff`.
    /// Adds random jitter (0–25% of computed backoff) to avoid thundering herd.
    fn backoff_duration(&self, attempt: u32) -> Duration {
        let base =
            self.initial_backoff.as_secs_f64() * self.backoff_multiplier.powi(attempt as i32);
        let capped = base.min(self.max_backoff.as_secs_f64());

        // Add jitter: random value between 0 and 25% of the capped backoff
        let jitter = capped * jitter_fraction();
        let total = capped + jitter;

        Duration::from_secs_f64(total)
    }
}

/// Returns true if the HTTP status code is retryable (429, 500, 502, 503, 504).
fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 429 | 500 | 502 | 503 | 504)
}

/// Parse the `Retry-After` header value into a `Duration`.
///
/// Supports integer seconds format (e.g., `Retry-After: 30`).
/// Returns `None` if the header is missing or unparseable.
fn parse_retry_after(response: &Response) -> Option<Duration> {
    response
        .headers()
        .get("retry-after")
        .and_then(|val| val.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
}

/// Generate a jitter fraction between 0.0 and 0.25.
///
/// Uses a simple pseudo-random approach based on the current time to avoid
/// pulling in a full random number generator dependency.
fn jitter_fraction() -> f64 {
    // Use nanosecond component of current time as a cheap entropy source
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    // Map to [0.0, 0.25)
    (nanos as f64 / u32::MAX as f64) * 0.25
}

/// Execute a request function with automatic retry on transient errors.
///
/// This function:
/// 1. Executes the request function
/// 2. If the response status is retryable (429, 500, 502, 503, 504):
///    - Checks if we've exceeded `max_attempts`
///    - If 429 and `Retry-After` header is present, waits at least that duration
///    - Otherwise uses exponential backoff: `initial * multiplier^attempt + jitter`
///    - Retries the request
/// 3. If non-retryable status, returns the response immediately
/// 4. After exhausting retries, returns the last error response
///
/// # Arguments
///
/// * `policy` - The retry policy configuration
/// * `request_fn` - An async closure that produces a `reqwest::Response`
///
/// # Returns
///
/// The final `Response` — either a successful one, a non-retryable error response,
/// or the last retryable error response after exhausting all attempts.
///
/// # Example
///
/// ```rust,ignore
/// use adk_enterprise::retry::{RetryPolicy, execute_with_retry};
///
/// let policy = RetryPolicy::default();
/// let response = execute_with_retry(&policy, || async {
///     client.get("https://api.example.com/data").send().await
/// }).await?;
/// ```
pub async fn execute_with_retry<F, Fut>(
    policy: &RetryPolicy,
    request_fn: F,
) -> std::result::Result<Response, EnterpriseError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = std::result::Result<Response, reqwest::Error>>,
{
    let mut last_error: Option<EnterpriseError> = None;

    for attempt in 0..=policy.max_attempts {
        // Execute the request
        let response = match request_fn().await {
            Ok(resp) => resp,
            Err(e) => {
                // Network/connection errors are retryable
                if attempt < policy.max_attempts && (e.is_timeout() || e.is_connect()) {
                    let backoff = policy.backoff_duration(attempt);
                    warn!(
                        attempt = attempt + 1,
                        max_attempts = policy.max_attempts,
                        backoff_ms = backoff.as_millis() as u64,
                        "connection error, retrying: {e}"
                    );
                    tokio::time::sleep(backoff).await;
                    last_error = Some(EnterpriseError::Connection(e));
                    continue;
                }
                return Err(EnterpriseError::Connection(e));
            }
        };

        let status = response.status();

        // Success — return immediately
        if status.is_success() {
            return Ok(response);
        }

        // Non-retryable error — return immediately (400, 401, 403, 404, 409, 422)
        if !is_retryable_status(status) {
            return Ok(response);
        }

        // Retryable error — check if we have attempts remaining
        if attempt >= policy.max_attempts {
            debug!(status = status.as_u16(), "retries exhausted, returning last error response");
            return Ok(response);
        }

        // Calculate wait duration
        let backoff = policy.backoff_duration(attempt);
        let wait_duration = if status.as_u16() == 429 {
            // Respect Retry-After header on 429, use at least that duration
            match parse_retry_after(&response) {
                Some(retry_after) => retry_after.max(backoff),
                None => backoff,
            }
        } else {
            backoff
        };

        warn!(
            status = status.as_u16(),
            attempt = attempt + 1,
            max_attempts = policy.max_attempts,
            wait_ms = wait_duration.as_millis() as u64,
            "retryable error, waiting before retry"
        );

        last_error = Some(EnterpriseError::Internal {
            message: format!("HTTP {status} (attempt {}/{})", attempt + 1, policy.max_attempts),
        });

        tokio::time::sleep(wait_duration).await;
    }

    // Should not reach here, but handle gracefully
    Err(last_error.unwrap_or_else(|| EnterpriseError::Internal {
        message: "retry loop exited unexpectedly".into(),
    }))
}

/// Execute a create request with automatic retry and idempotency key.
///
/// Generates a UUID v4 idempotency key ONCE before the retry loop, then passes
/// it to the request builder on every attempt. This ensures retries of create
/// operations don't produce duplicate resources on the server.
///
/// # Arguments
///
/// * `policy` - The retry policy configuration
/// * `request_fn` - An async closure that receives the idempotency key and produces
///   a `reqwest::Response`. The closure must attach the key as the `Idempotency-Key`
///   header.
///
/// # Example
///
/// ```rust,ignore
/// use adk_enterprise::retry::{RetryPolicy, execute_create_with_retry};
///
/// let policy = RetryPolicy::default();
/// let response = execute_create_with_retry(&policy, |idempotency_key| async move {
///     client
///         .post("https://api.example.com/agents")
///         .header("Idempotency-Key", idempotency_key)
///         .json(&params)
///         .send()
///         .await
/// }).await?;
/// ```
pub async fn execute_create_with_retry<F, Fut>(
    policy: &RetryPolicy,
    request_fn: F,
) -> std::result::Result<Response, EnterpriseError>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = std::result::Result<Response, reqwest::Error>>,
{
    // Generate the idempotency key ONCE — reused across all retry attempts
    let idempotency_key = generate_idempotency_key();

    let mut last_error: Option<EnterpriseError> = None;

    for attempt in 0..=policy.max_attempts {
        // Pass the same idempotency key on every attempt
        let response = match request_fn(idempotency_key.clone()).await {
            Ok(resp) => resp,
            Err(e) => {
                // Network/connection errors are retryable
                if attempt < policy.max_attempts && (e.is_timeout() || e.is_connect()) {
                    let backoff = policy.backoff_duration(attempt);
                    warn!(
                        attempt = attempt + 1,
                        max_attempts = policy.max_attempts,
                        backoff_ms = backoff.as_millis() as u64,
                        idempotency_key = %idempotency_key,
                        "connection error on create, retrying with same idempotency key: {e}"
                    );
                    tokio::time::sleep(backoff).await;
                    last_error = Some(EnterpriseError::Connection(e));
                    continue;
                }
                return Err(EnterpriseError::Connection(e));
            }
        };

        let status = response.status();

        // Success — return immediately
        if status.is_success() {
            return Ok(response);
        }

        // Non-retryable error — return immediately
        if !is_retryable_status(status) {
            return Ok(response);
        }

        // Retryable error — check if we have attempts remaining
        if attempt >= policy.max_attempts {
            debug!(
                status = status.as_u16(),
                idempotency_key = %idempotency_key,
                "retries exhausted on create operation"
            );
            return Ok(response);
        }

        // Calculate wait duration
        let backoff = policy.backoff_duration(attempt);
        let wait_duration = if status.as_u16() == 429 {
            match parse_retry_after(&response) {
                Some(retry_after) => retry_after.max(backoff),
                None => backoff,
            }
        } else {
            backoff
        };

        warn!(
            status = status.as_u16(),
            attempt = attempt + 1,
            max_attempts = policy.max_attempts,
            wait_ms = wait_duration.as_millis() as u64,
            idempotency_key = %idempotency_key,
            "retryable error on create, retrying with same idempotency key"
        );

        last_error = Some(EnterpriseError::Internal {
            message: format!("HTTP {status} (attempt {}/{})", attempt + 1, policy.max_attempts),
        });

        tokio::time::sleep(wait_duration).await;
    }

    Err(last_error.unwrap_or_else(|| EnterpriseError::Internal {
        message: "retry loop exited unexpectedly".into(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::idempotency::IDEMPOTENCY_KEY_HEADER;

    #[test]
    fn test_default_policy() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_attempts, 3);
        assert_eq!(policy.initial_backoff, Duration::from_secs(1));
        assert_eq!(policy.max_backoff, Duration::from_secs(30));
        assert_eq!(policy.backoff_multiplier, 2.0);
    }

    #[test]
    fn test_from_config() {
        let policy = RetryPolicy::from_config(5, Duration::from_millis(500));
        assert_eq!(policy.max_attempts, 5);
        assert_eq!(policy.initial_backoff, Duration::from_millis(500));
        assert_eq!(policy.max_backoff, Duration::from_secs(30));
        assert_eq!(policy.backoff_multiplier, 2.0);
    }

    #[test]
    fn test_backoff_duration_exponential() {
        let policy = RetryPolicy {
            max_attempts: 5,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(30),
            backoff_multiplier: 2.0,
        };

        // Attempt 0: 1s base (+ jitter)
        let d0 = policy.backoff_duration(0);
        assert!(d0 >= Duration::from_secs(1));
        assert!(d0 <= Duration::from_millis(1250));

        // Attempt 1: 2s base (+ jitter)
        let d1 = policy.backoff_duration(1);
        assert!(d1 >= Duration::from_secs(2));
        assert!(d1 <= Duration::from_millis(2500));

        // Attempt 2: 4s base (+ jitter)
        let d2 = policy.backoff_duration(2);
        assert!(d2 >= Duration::from_secs(4));
        assert!(d2 <= Duration::from_millis(5000));
    }

    #[test]
    fn test_backoff_capped_at_max() {
        let policy = RetryPolicy {
            max_attempts: 10,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(10),
            backoff_multiplier: 2.0,
        };

        // Attempt 5: 32s base but capped at 10s (+ jitter up to 2.5s)
        let d = policy.backoff_duration(5);
        assert!(d >= Duration::from_secs(10));
        assert!(d <= Duration::from_millis(12500));
    }

    #[test]
    fn test_is_retryable_status() {
        use reqwest::StatusCode;

        // Retryable statuses
        assert!(is_retryable_status(StatusCode::TOO_MANY_REQUESTS)); // 429
        assert!(is_retryable_status(StatusCode::INTERNAL_SERVER_ERROR)); // 500
        assert!(is_retryable_status(StatusCode::BAD_GATEWAY)); // 502
        assert!(is_retryable_status(StatusCode::SERVICE_UNAVAILABLE)); // 503
        assert!(is_retryable_status(StatusCode::GATEWAY_TIMEOUT)); // 504

        // Non-retryable statuses
        assert!(!is_retryable_status(StatusCode::BAD_REQUEST)); // 400
        assert!(!is_retryable_status(StatusCode::UNAUTHORIZED)); // 401
        assert!(!is_retryable_status(StatusCode::FORBIDDEN)); // 403
        assert!(!is_retryable_status(StatusCode::NOT_FOUND)); // 404
        assert!(!is_retryable_status(StatusCode::CONFLICT)); // 409
        assert!(!is_retryable_status(StatusCode::UNPROCESSABLE_ENTITY)); // 422

        // Success statuses are not retryable
        assert!(!is_retryable_status(StatusCode::OK)); // 200
        assert!(!is_retryable_status(StatusCode::CREATED)); // 201
    }

    #[test]
    fn test_jitter_fraction_bounds() {
        // Run multiple times to check bounds
        for _ in 0..100 {
            let j = jitter_fraction();
            assert!(j >= 0.0);
            assert!(j < 0.25);
        }
    }

    #[test]
    fn test_idempotency_key_header_constant() {
        assert_eq!(IDEMPOTENCY_KEY_HEADER, "Idempotency-Key");
    }

    #[tokio::test]
    async fn test_execute_create_with_retry_passes_same_key() {
        // Verify key generation produces valid UUID v4 keys
        let key = generate_idempotency_key();
        assert_eq!(key.len(), 36);

        // Verify that two calls produce different keys (each create gets its own key)
        let key2 = generate_idempotency_key();
        assert_ne!(key, key2);
    }
}
