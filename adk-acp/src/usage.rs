//! Usage tracking for ACP agent invocations.
//!
//! Records per-call metrics so you can monitor costs and performance
//! across both your LLM provider (token costs) and ACP agent (credits/time).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Usage metrics from a single ACP agent invocation.
#[derive(Debug, Clone)]
pub struct AcpUsage {
    /// Name of the ACP agent tool that was invoked.
    pub tool_name: String,
    /// Length of the prompt sent (characters).
    pub prompt_chars: usize,
    /// Length of the response received (characters).
    pub response_chars: usize,
    /// Wall-clock duration of the invocation.
    pub duration: Duration,
    /// Whether the invocation succeeded.
    pub success: bool,
    /// Number of permission requests received during this invocation.
    pub permission_requests: u32,
    /// Number of permission requests that were denied.
    pub permissions_denied: u32,
}

/// Aggregated usage statistics across all ACP invocations.
#[derive(Debug, Clone)]
pub struct AcpUsageStats {
    /// Total number of invocations.
    pub total_calls: u64,
    /// Total successful invocations.
    pub successful_calls: u64,
    /// Total failed invocations.
    pub failed_calls: u64,
    /// Total prompt characters sent.
    pub total_prompt_chars: u64,
    /// Total response characters received.
    pub total_response_chars: u64,
    /// Total wall-clock time spent in ACP calls.
    pub total_duration: Duration,
    /// Total permission requests received.
    pub total_permission_requests: u64,
    /// Total permission requests denied.
    pub total_permissions_denied: u64,
}

/// Thread-safe usage tracker for ACP invocations.
///
/// Attach to an `AcpAgentTool` or use standalone to aggregate metrics.
///
/// # Example
///
/// ```rust,ignore
/// use adk_acp::usage::UsageTracker;
///
/// let tracker = UsageTracker::new();
/// // ... tool invocations happen ...
/// let stats = tracker.stats();
/// println!("Total ACP calls: {}", stats.total_calls);
/// println!("Avg response time: {:?}", stats.total_duration / stats.total_calls as u32);
/// ```
#[derive(Debug, Clone, Default)]
pub struct UsageTracker {
    inner: Arc<UsageTrackerInner>,
}

#[derive(Debug, Default)]
struct UsageTrackerInner {
    total_calls: AtomicU64,
    successful_calls: AtomicU64,
    failed_calls: AtomicU64,
    total_prompt_chars: AtomicU64,
    total_response_chars: AtomicU64,
    total_duration_ms: AtomicU64,
    total_permission_requests: AtomicU64,
    total_permissions_denied: AtomicU64,
}

impl UsageTracker {
    /// Create a new usage tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a completed ACP invocation.
    pub fn record(&self, usage: &AcpUsage) {
        let inner = &self.inner;
        inner.total_calls.fetch_add(1, Ordering::Relaxed);
        if usage.success {
            inner.successful_calls.fetch_add(1, Ordering::Relaxed);
        } else {
            inner.failed_calls.fetch_add(1, Ordering::Relaxed);
        }
        inner.total_prompt_chars.fetch_add(usage.prompt_chars as u64, Ordering::Relaxed);
        inner.total_response_chars.fetch_add(usage.response_chars as u64, Ordering::Relaxed);
        inner.total_duration_ms.fetch_add(usage.duration.as_millis() as u64, Ordering::Relaxed);
        inner
            .total_permission_requests
            .fetch_add(u64::from(usage.permission_requests), Ordering::Relaxed);
        inner
            .total_permissions_denied
            .fetch_add(u64::from(usage.permissions_denied), Ordering::Relaxed);
    }

    /// Get aggregated usage statistics.
    pub fn stats(&self) -> AcpUsageStats {
        let inner = &self.inner;
        AcpUsageStats {
            total_calls: inner.total_calls.load(Ordering::Relaxed),
            successful_calls: inner.successful_calls.load(Ordering::Relaxed),
            failed_calls: inner.failed_calls.load(Ordering::Relaxed),
            total_prompt_chars: inner.total_prompt_chars.load(Ordering::Relaxed),
            total_response_chars: inner.total_response_chars.load(Ordering::Relaxed),
            total_duration: Duration::from_millis(inner.total_duration_ms.load(Ordering::Relaxed)),
            total_permission_requests: inner.total_permission_requests.load(Ordering::Relaxed),
            total_permissions_denied: inner.total_permissions_denied.load(Ordering::Relaxed),
        }
    }

    /// Reset all counters to zero.
    pub fn reset(&self) {
        let inner = &self.inner;
        inner.total_calls.store(0, Ordering::Relaxed);
        inner.successful_calls.store(0, Ordering::Relaxed);
        inner.failed_calls.store(0, Ordering::Relaxed);
        inner.total_prompt_chars.store(0, Ordering::Relaxed);
        inner.total_response_chars.store(0, Ordering::Relaxed);
        inner.total_duration_ms.store(0, Ordering::Relaxed);
        inner.total_permission_requests.store(0, Ordering::Relaxed);
        inner.total_permissions_denied.store(0, Ordering::Relaxed);
    }
}
