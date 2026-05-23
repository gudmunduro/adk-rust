//! Property tests for deferred node (fan-in barrier) completeness.
//!
//! **Feature: runtime-reliability-sprint, Property 3: Fan-In Completeness**
//! *For any* deferred node with N upstream paths, the node SHALL NOT execute
//! until all N paths have produced output OR the fan_in_timeout has expired.
//! **Validates: Requirements 6.2, 8.2**

use adk_graph::deferred::{FanInTracker, MergeStrategy};
use proptest::prelude::*;
use serde_json::{Value, json};

// ── Generators ────────────────────────────────────────────────────────

/// Generate a random number of upstream paths (2..=5).
fn arb_num_paths() -> impl Strategy<Value = usize> {
    2usize..=5usize
}

/// Generate a random JSON value for node output.
fn arb_output_value() -> impl Strategy<Value = Value> {
    prop_oneof![
        any::<i64>().prop_map(|n| json!(n)),
        "[a-z]{1,10}".prop_map(|s| json!(s)),
        any::<bool>().prop_map(|b| json!(b)),
        (any::<i64>(), "[a-z]{1,5}").prop_map(|(n, k)| json!({ k: n })),
    ]
}

// ── Property 3: Fan-In Completeness ───────────────────────────────────
//
// **Feature: runtime-reliability-sprint, Property 3: Fan-In Completeness**
// *For any* deferred node with N upstream paths (2..=5), the tracker SHALL NOT
// report `is_ready()` until all N paths have produced output.
// **Validates: Requirements 6.2, 8.2**

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// For any N upstream paths, `is_ready()` is false until all N have reported.
    #[test]
    fn prop_fan_in_not_ready_until_all_complete(
        num_paths in arb_num_paths(),
        outputs in proptest::collection::vec(arb_output_value(), 5),
    ) {
        // Generate deterministic source names based on num_paths
        let sources: Vec<String> = (0..num_paths).map(|i| format!("source_{i}")).collect();
        let source_refs: Vec<&str> = sources.iter().map(|s| s.as_str()).collect();

        let mut tracker = FanInTracker::new(source_refs);

        // Record outputs one by one, asserting is_ready() is false until all are recorded
        for i in 0..num_paths {
            prop_assert!(
                !tracker.is_ready(),
                "tracker should NOT be ready after {}/{} sources recorded",
                i,
                num_paths
            );
            prop_assert_eq!(
                tracker.received_count(),
                i,
                "received_count should be {} before recording source {}",
                i,
                i
            );

            tracker.record(&sources[i], outputs[i % outputs.len()].clone());
        }

        // After all are recorded, assert is_ready() is true
        prop_assert!(
            tracker.is_ready(),
            "tracker should be ready after all {} sources recorded",
            num_paths
        );
        prop_assert_eq!(tracker.received_count(), num_paths);
    }
}

// ── Property 3b: MergeStrategy::Collect produces Vec with all outputs ─

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// MergeStrategy::Collect produces a Vec containing all outputs in insertion order.
    #[test]
    fn prop_merge_collect_produces_ordered_vec(
        num_paths in arb_num_paths(),
        outputs in proptest::collection::vec(arb_output_value(), 2..=5),
    ) {
        let sources: Vec<String> = (0..num_paths).map(|i| format!("src_{i}")).collect();
        let source_refs: Vec<&str> = sources.iter().map(|s| s.as_str()).collect();

        let mut tracker = FanInTracker::new(source_refs);

        // Record all outputs
        let mut expected_outputs = Vec::new();
        for i in 0..num_paths {
            let output = outputs[i % outputs.len()].clone();
            expected_outputs.push(output.clone());
            tracker.record(&sources[i], output);
        }

        prop_assert!(tracker.is_ready());

        let merged = tracker.merge(&MergeStrategy::Collect);

        // Verify it's an array with the correct length
        let arr = merged.as_array().expect("Collect should produce an array");
        prop_assert_eq!(
            arr.len(),
            num_paths,
            "Collect should produce {} elements, got {}",
            num_paths,
            arr.len()
        );

        // Verify insertion order is preserved
        for (i, expected) in expected_outputs.iter().enumerate() {
            prop_assert_eq!(
                &arr[i],
                expected,
                "Element at index {} should match recorded output",
                i
            );
        }
    }
}

// ── Property 3c: MergeStrategy::MergeMap merges all object outputs ────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// MergeStrategy::MergeMap merges all object outputs correctly (last-write-wins).
    #[test]
    fn prop_merge_map_combines_all_keys(
        num_paths in arb_num_paths(),
        values in proptest::collection::vec(any::<i64>(), 5),
    ) {
        let sources: Vec<String> = (0..num_paths).map(|i| format!("node_{i}")).collect();
        let source_refs: Vec<&str> = sources.iter().map(|s| s.as_str()).collect();

        let mut tracker = FanInTracker::new(source_refs);

        // Each source produces a unique key
        for i in 0..num_paths {
            let key = format!("key_{i}");
            let output = json!({ key: values[i % values.len()] });
            tracker.record(&sources[i], output);
        }

        prop_assert!(tracker.is_ready());

        let merged = tracker.merge(&MergeStrategy::MergeMap);
        let obj = merged.as_object().expect("MergeMap should produce an object");

        // Verify all unique keys are present
        for i in 0..num_paths {
            let key = format!("key_{i}");
            prop_assert!(
                obj.contains_key(&key),
                "merged object should contain key '{}'",
                key
            );
            let expected_val = json!(values[i % values.len()]);
            prop_assert_eq!(
                obj.get(&key).unwrap(),
                &expected_val,
                "value for key '{}' should match",
                key
            );
        }
    }
}

// ── Property 3d: MergeStrategy::First returns only the first recorded output ─

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// MergeStrategy::First returns only the first recorded output.
    #[test]
    fn prop_merge_first_returns_first_recorded(
        num_paths in arb_num_paths(),
        outputs in proptest::collection::vec(arb_output_value(), 2..=5),
    ) {
        let sources: Vec<String> = (0..num_paths).map(|i| format!("path_{i}")).collect();
        let source_refs: Vec<&str> = sources.iter().map(|s| s.as_str()).collect();

        let mut tracker = FanInTracker::new(source_refs);

        let first_output = outputs[0].clone();

        // Record all outputs
        for i in 0..num_paths {
            tracker.record(&sources[i], outputs[i % outputs.len()].clone());
        }

        prop_assert!(tracker.is_ready());

        let merged = tracker.merge(&MergeStrategy::First);

        // First strategy should return the first recorded output
        prop_assert_eq!(
            &merged,
            &first_output,
            "First strategy should return the first recorded output"
        );
    }
}

// ── Property 3e: Expected count matches construction parameter ────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// The expected_count always matches the number of sources provided at construction.
    #[test]
    fn prop_expected_count_matches_construction(
        num_paths in arb_num_paths(),
    ) {
        let sources: Vec<String> = (0..num_paths).map(|i| format!("s_{i}")).collect();
        let source_refs: Vec<&str> = sources.iter().map(|s| s.as_str()).collect();

        let tracker = FanInTracker::new(source_refs);

        prop_assert_eq!(
            tracker.expected_count(),
            num_paths,
            "expected_count should equal the number of sources"
        );
    }
}
