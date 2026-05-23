//! Property-based tests for A2A interceptor chain execution order.
//!
//! Tests correctness property 7 from the design document:
//! - Property 7: Interceptor Chain Order
//!
//! For any chain of N interceptors, `before_delegation` SHALL execute in
//! registration order (1..N) and `after_delegation` SHALL execute in reverse
//! order (N..1).

#![cfg(feature = "a2a-interceptors")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use proptest::prelude::*;

use adk_server::a2a::interceptor::{
    A2aDelegationContext, A2aError, A2aInterceptor, InterceptorChain, InterceptorDecision,
};

// ---------------------------------------------------------------------------
// Test interceptor that records execution order
// ---------------------------------------------------------------------------

/// An interceptor that records its ID and phase into a shared log.
struct OrderRecordingInterceptor {
    id: usize,
    log: Arc<Mutex<Vec<(usize, &'static str)>>>,
}

#[async_trait]
impl A2aInterceptor for OrderRecordingInterceptor {
    async fn before_delegation(
        &self,
        _ctx: &mut A2aDelegationContext,
    ) -> Result<InterceptorDecision, A2aError> {
        self.log.lock().unwrap().push((self.id, "before"));
        Ok(InterceptorDecision::Continue)
    }

    async fn after_delegation(
        &self,
        _ctx: &A2aDelegationContext,
        _response: &mut serde_json::Value,
    ) -> Result<(), A2aError> {
        self.log.lock().unwrap().push((self.id, "after"));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a tokio runtime for running async tests inside proptest.
fn run_async<F: std::future::Future<Output = Result<(), TestCaseError>>>(
    f: F,
) -> Result<(), TestCaseError> {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
}

/// Create a default A2aDelegationContext for testing.
fn make_ctx() -> A2aDelegationContext {
    A2aDelegationContext {
        method: "tasks/send".to_string(),
        params: serde_json::json!({}),
        caller_id: None,
        metadata: HashMap::new(),
    }
}

// ---------------------------------------------------------------------------
// Property 7: Interceptor Chain Order
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Feature: runtime-reliability-sprint, Property 7: Interceptor Chain Order**
    ///
    /// *For any* chain of N interceptors, `before_delegation` SHALL execute in
    /// registration order (1..N) and `after_delegation` SHALL execute in reverse
    /// order (N..1).
    ///
    /// **Validates: Requirements 14.2, 14.3**
    #[test]
    fn prop_interceptor_chain_order(n in 1usize..=10) {
        run_async(async {
            let log = Arc::new(Mutex::new(Vec::new()));

            // Build a chain with N interceptors, each with ID 1..=N
            let mut chain = InterceptorChain::new();
            for i in 1..=n {
                chain = chain.add(OrderRecordingInterceptor { id: i, log: log.clone() });
            }

            // Verify chain length
            prop_assert_eq!(
                chain.len(),
                n,
                "chain should contain exactly N interceptors"
            );

            // Run before_delegation and verify order is 1..N
            let mut ctx = make_ctx();
            let decision = chain.run_before(&mut ctx).await
                .map_err(|e| TestCaseError::fail(format!("run_before failed: {e}")))?;

            prop_assert!(
                matches!(decision, InterceptorDecision::Continue),
                "all interceptors return Continue, so chain should return Continue"
            );

            {
                let recorded = log.lock().unwrap();
                let expected_before: Vec<(usize, &str)> =
                    (1..=n).map(|i| (i, "before")).collect();
                prop_assert_eq!(
                    recorded.as_slice(),
                    expected_before.as_slice(),
                    "before_delegation should execute in registration order 1..N"
                );
            }

            // Clear the log for after_delegation test
            log.lock().unwrap().clear();

            // Run after_delegation and verify order is N..1
            let mut response = serde_json::json!({"result": "ok"});
            chain.run_after(&ctx, &mut response).await
                .map_err(|e| TestCaseError::fail(format!("run_after failed: {e}")))?;

            {
                let recorded = log.lock().unwrap();
                let expected_after: Vec<(usize, &str)> =
                    (1..=n).rev().map(|i| (i, "after")).collect();
                prop_assert_eq!(
                    recorded.as_slice(),
                    expected_after.as_slice(),
                    "after_delegation should execute in reverse order N..1"
                );
            }

            Ok(())
        })?;
    }
}
