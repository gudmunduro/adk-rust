//! Property-based tests for tool concurrency enforcement.
//!
//! **Feature: runtime-reliability-sprint, Property 1: Tool Concurrency Invariant**
//! *For any* set of concurrent tool calls with `max_concurrency = K`, at no point
//! in time SHALL more than K tool executions be active simultaneously.
//!
//! **Validates: Requirements 1.4, 2.2**

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use adk_core::{BackpressurePolicy, ToolConcurrencyConfig, ToolConcurrencyManager};
use proptest::prelude::*;
use tokio::runtime::Builder;

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

/// Generate a concurrency limit K in [1, 10].
fn arb_concurrency_limit() -> impl Strategy<Value = usize> {
    1usize..=10
}

/// Generate a number of concurrent calls N in [2, 50].
fn arb_concurrent_calls() -> impl Strategy<Value = usize> {
    2usize..=50
}

/// Generate a per-tool concurrency limit in [1, 5].
fn arb_per_tool_limit() -> impl Strategy<Value = usize> {
    1usize..=5
}

// ---------------------------------------------------------------------------
// Property Tests
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Feature: runtime-reliability-sprint, Property 1: Tool Concurrency Invariant**
    /// *For any* N concurrent tool calls with global `max_concurrency = K`,
    /// at no point in time SHALL more than K tool executions be active simultaneously.
    /// **Validates: Requirements 1.4, 2.2**
    #[test]
    fn prop_global_concurrency_never_exceeds_limit(
        k in arb_concurrency_limit(),
        n in arb_concurrent_calls(),
    ) {
        let rt = Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let config = ToolConcurrencyConfig {
                max_concurrency: Some(k),
                per_tool: HashMap::new(),
                backpressure: BackpressurePolicy::Queue,
            };
            let manager = Arc::new(ToolConcurrencyManager::new(&config));

            // Shared atomic counter tracking active tool calls
            let active = Arc::new(AtomicUsize::new(0));
            // Track the maximum observed active count
            let max_active = Arc::new(AtomicUsize::new(0));

            let mut handles = Vec::with_capacity(n);

            for _ in 0..n {
                let mgr = manager.clone();
                let active = active.clone();
                let max_active = max_active.clone();

                handles.push(tokio::spawn(async move {
                    let _permit = mgr.acquire("test_tool").await.unwrap();

                    // Increment active counter
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;

                    // Update max observed
                    max_active.fetch_max(current, Ordering::SeqCst);

                    // Assert invariant: active count must never exceed K
                    assert!(
                        current <= k,
                        "concurrency invariant violated: {current} active > limit {k}"
                    );

                    // Simulate brief work (1ms)
                    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;

                    // Decrement active counter
                    active.fetch_sub(1, Ordering::SeqCst);

                    // Permit dropped here, releasing the semaphore
                }));
            }

            // Wait for all tasks to complete
            for handle in handles {
                handle.await.unwrap();
            }

            // After all tasks complete, active count must be 0
            let final_active = active.load(Ordering::SeqCst);
            assert_eq!(
                final_active, 0,
                "active counter not zero after all tasks completed: {final_active}"
            );

            // Verify that the max observed active count was at most K
            let observed_max = max_active.load(Ordering::SeqCst);
            assert!(
                observed_max <= k,
                "max observed active {observed_max} exceeded limit {k}"
            );
        });
    }

    /// **Feature: runtime-reliability-sprint, Property 1: Per-Tool Concurrency Invariant**
    /// *For any* per-tool concurrency override with limit L, at no point in time
    /// SHALL more than L executions of that tool be active simultaneously.
    /// **Validates: Requirements 1.4, 2.2**
    #[test]
    fn prop_per_tool_concurrency_never_exceeds_limit(
        tool_limit in arb_per_tool_limit(),
        n in arb_concurrent_calls(),
    ) {
        let rt = Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let config = ToolConcurrencyConfig {
                max_concurrency: Some(50), // high global limit
                per_tool: HashMap::from([("limited_tool".to_string(), tool_limit)]),
                backpressure: BackpressurePolicy::Queue,
            };
            let manager = Arc::new(ToolConcurrencyManager::new(&config));

            let active = Arc::new(AtomicUsize::new(0));
            let max_active = Arc::new(AtomicUsize::new(0));

            let mut handles = Vec::with_capacity(n);

            for _ in 0..n {
                let mgr = manager.clone();
                let active = active.clone();
                let max_active = max_active.clone();

                handles.push(tokio::spawn(async move {
                    let _permit = mgr.acquire("limited_tool").await.unwrap();

                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_active.fetch_max(current, Ordering::SeqCst);

                    assert!(
                        current <= tool_limit,
                        "per-tool concurrency violated: {current} active > limit {tool_limit}"
                    );

                    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;

                    active.fetch_sub(1, Ordering::SeqCst);
                }));
            }

            for handle in handles {
                handle.await.unwrap();
            }

            let final_active = active.load(Ordering::SeqCst);
            assert_eq!(final_active, 0, "active counter not zero: {final_active}");

            let observed_max = max_active.load(Ordering::SeqCst);
            assert!(
                observed_max <= tool_limit,
                "max observed {observed_max} exceeded per-tool limit {tool_limit}"
            );
        });
    }

    /// **Feature: runtime-reliability-sprint, Property 1: Multiple Per-Tool Limits**
    /// *For any* set of per-tool overrides, each tool's concurrent count SHALL
    /// never exceed its individual limit.
    /// **Validates: Requirements 1.4, 2.2**
    #[test]
    fn prop_multiple_per_tool_limits_enforced_independently(
        limit_a in arb_per_tool_limit(),
        limit_b in arb_per_tool_limit(),
        n in 4usize..=30,
    ) {
        let rt = Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let config = ToolConcurrencyConfig {
                max_concurrency: None, // no global limit
                per_tool: HashMap::from([
                    ("tool_a".to_string(), limit_a),
                    ("tool_b".to_string(), limit_b),
                ]),
                backpressure: BackpressurePolicy::Queue,
            };
            let manager = Arc::new(ToolConcurrencyManager::new(&config));

            let active_a = Arc::new(AtomicUsize::new(0));
            let active_b = Arc::new(AtomicUsize::new(0));
            let max_a = Arc::new(AtomicUsize::new(0));
            let max_b = Arc::new(AtomicUsize::new(0));

            let mut handles = Vec::with_capacity(n * 2);

            // Spawn N tasks for tool_a
            for _ in 0..n {
                let mgr = manager.clone();
                let active = active_a.clone();
                let max_active = max_a.clone();
                let limit = limit_a;

                handles.push(tokio::spawn(async move {
                    let _permit = mgr.acquire("tool_a").await.unwrap();

                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_active.fetch_max(current, Ordering::SeqCst);

                    assert!(
                        current <= limit,
                        "tool_a concurrency violated: {current} > {limit}"
                    );

                    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                }));
            }

            // Spawn N tasks for tool_b
            for _ in 0..n {
                let mgr = manager.clone();
                let active = active_b.clone();
                let max_active = max_b.clone();
                let limit = limit_b;

                handles.push(tokio::spawn(async move {
                    let _permit = mgr.acquire("tool_b").await.unwrap();

                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_active.fetch_max(current, Ordering::SeqCst);

                    assert!(
                        current <= limit,
                        "tool_b concurrency violated: {current} > {limit}"
                    );

                    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                }));
            }

            for handle in handles {
                handle.await.unwrap();
            }

            let final_a = active_a.load(Ordering::SeqCst);
            let final_b = active_b.load(Ordering::SeqCst);
            assert_eq!(final_a, 0, "tool_a active not zero: {final_a}");
            assert_eq!(final_b, 0, "tool_b active not zero: {final_b}");

            let obs_max_a = max_a.load(Ordering::SeqCst);
            let obs_max_b = max_b.load(Ordering::SeqCst);
            assert!(obs_max_a <= limit_a, "tool_a max {obs_max_a} > limit {limit_a}");
            assert!(obs_max_b <= limit_b, "tool_b max {obs_max_b} > limit {limit_b}");
        });
    }
}
