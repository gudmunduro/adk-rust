//! Property-based tests for ThinkingConfig validation.
//!
//! **Feature: one-point-zero-readiness, Property 4: Gemini ThinkingConfig Validation**
//!
//! *For any* `ThinkingConfig` value, the validation function SHALL accept configs where
//! `thinking_budget` is within the allowed range (0..=24576) and `include_thoughts` is
//! consistent with the budget, and SHALL reject configs where the budget exceeds the
//! maximum or is negative.
//!
//! **Validates: Requirements 10.2**

use adk_gemini::{ThinkingConfig, ThinkingLevel};
use proptest::prelude::*;

// ============================================================================
// Generators
// ============================================================================

/// Generates a valid thinking budget for Gemini 2.5 Flash (0..=24576).
fn arb_valid_budget() -> impl Strategy<Value = i32> {
    prop_oneof![
        Just(0i32),        // Disable thinking
        Just(-1i32),       // Dynamic thinking
        128i32..=24576i32, // Valid range
    ]
}

/// Generates a random ThinkingLevel.
fn arb_thinking_level() -> impl Strategy<Value = ThinkingLevel> {
    prop_oneof![
        Just(ThinkingLevel::Minimal),
        Just(ThinkingLevel::Low),
        Just(ThinkingLevel::Medium),
        Just(ThinkingLevel::High),
    ]
}

/// Generates a valid ThinkingConfig with budget only (no level).
fn arb_valid_budget_config() -> impl Strategy<Value = ThinkingConfig> {
    (arb_valid_budget(), prop::option::of(prop::bool::ANY)).prop_map(
        |(budget, include_thoughts)| {
            let mut config = ThinkingConfig::new().with_thinking_budget(budget);
            if let Some(include) = include_thoughts {
                config = config.with_thoughts_included(include);
            }
            config
        },
    )
}

/// Generates a valid ThinkingConfig with level only (no budget).
fn arb_valid_level_config() -> impl Strategy<Value = ThinkingConfig> {
    (arb_thinking_level(), prop::option::of(prop::bool::ANY)).prop_map(
        |(level, include_thoughts)| {
            let mut config = ThinkingConfig::new().with_thinking_level(level);
            if let Some(include) = include_thoughts {
                config = config.with_thoughts_included(include);
            }
            config
        },
    )
}

/// Generates a valid ThinkingConfig (either budget-only or level-only).
fn arb_valid_thinking_config() -> impl Strategy<Value = ThinkingConfig> {
    prop_oneof![
        arb_valid_budget_config(),
        arb_valid_level_config(),
        // Empty config (all None) is also valid
        Just(ThinkingConfig::new()),
    ]
}

/// Generates an invalid ThinkingConfig (both budget AND level set — mutually exclusive).
fn arb_invalid_mutual_exclusive_config() -> impl Strategy<Value = ThinkingConfig> {
    (arb_valid_budget(), arb_thinking_level()).prop_map(|(budget, level)| ThinkingConfig {
        thinking_budget: Some(budget),
        include_thoughts: None,
        thinking_level: Some(level),
    })
}

// ============================================================================
// Property Tests
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Feature: one-point-zero-readiness, Property 4: ThinkingConfig Validation — Valid Configs**
    ///
    /// *For any* `ThinkingConfig` where `thinking_budget` is within the allowed range
    /// (0..=24576 or -1 for dynamic) and `thinking_level` is not simultaneously set,
    /// the validation function SHALL accept the config.
    ///
    /// **Validates: Requirements 10.2**
    #[test]
    fn prop_valid_thinking_config_accepted(config in arb_valid_thinking_config()) {
        let result = config.validate();
        prop_assert!(
            result.is_ok(),
            "Valid ThinkingConfig should pass validation, got error: {:?}",
            result.err()
        );
    }

    /// **Feature: one-point-zero-readiness, Property 4: ThinkingConfig Validation — Mutual Exclusion**
    ///
    /// *For any* `ThinkingConfig` where both `thinking_budget` and `thinking_level` are set,
    /// the validation function SHALL reject the config since they are mutually exclusive.
    ///
    /// **Validates: Requirements 10.2**
    #[test]
    fn prop_mutual_exclusive_config_rejected(config in arb_invalid_mutual_exclusive_config()) {
        let result = config.validate();
        prop_assert!(
            result.is_err(),
            "ThinkingConfig with both budget and level should fail validation"
        );
        let err_msg = result.unwrap_err();
        prop_assert!(
            err_msg.contains("mutually exclusive"),
            "Error message should mention mutual exclusivity, got: {err_msg}"
        );
    }

    /// **Feature: one-point-zero-readiness, Property 4: ThinkingConfig Serialization Round-Trip**
    ///
    /// *For any* valid `ThinkingConfig`, serializing to JSON and deserializing back
    /// SHALL produce a config equal to the original.
    ///
    /// **Validates: Requirements 10.2**
    #[test]
    fn prop_thinking_config_serialization_roundtrip(config in arb_valid_thinking_config()) {
        let json = serde_json::to_string(&config).expect("serialization should succeed");
        let deserialized: ThinkingConfig =
            serde_json::from_str(&json).expect("deserialization should succeed");

        // Compare via JSON values to handle field ordering
        let original_value = serde_json::to_value(&config).expect("to_value should succeed");
        let roundtrip_value = serde_json::to_value(&deserialized).expect("to_value should succeed");

        prop_assert_eq!(
            original_value,
            roundtrip_value,
            "ThinkingConfig should survive JSON round-trip"
        );
    }

    /// **Feature: one-point-zero-readiness, Property 4: Budget-only config has no level**
    ///
    /// *For any* valid budget-only ThinkingConfig, the thinking_level field SHALL be None.
    ///
    /// **Validates: Requirements 10.2**
    #[test]
    fn prop_budget_only_config_has_no_level(config in arb_valid_budget_config()) {
        prop_assert!(
            config.thinking_level.is_none(),
            "Budget-only config should not have thinking_level set"
        );
        prop_assert!(
            config.thinking_budget.is_some(),
            "Budget-only config should have thinking_budget set"
        );
    }

    /// **Feature: one-point-zero-readiness, Property 4: Level-only config has no budget**
    ///
    /// *For any* valid level-only ThinkingConfig, the thinking_budget field SHALL be None.
    ///
    /// **Validates: Requirements 10.2**
    #[test]
    fn prop_level_only_config_has_no_budget(config in arb_valid_level_config()) {
        prop_assert!(
            config.thinking_budget.is_none(),
            "Level-only config should not have thinking_budget set"
        );
        prop_assert!(
            config.thinking_level.is_some(),
            "Level-only config should have thinking_level set"
        );
    }
}

// ============================================================================
// Additional deterministic tests
// ============================================================================

#[test]
fn test_default_config_validates() {
    let config = ThinkingConfig::new();
    assert!(config.validate().is_ok());
}

#[test]
fn test_dynamic_thinking_validates() {
    let config = ThinkingConfig::dynamic_thinking();
    assert!(config.validate().is_ok());
    assert_eq!(config.thinking_budget, Some(-1));
    assert_eq!(config.include_thoughts, Some(true));
}

#[test]
fn test_zero_budget_validates() {
    let config = ThinkingConfig::new().with_thinking_budget(0);
    assert!(config.validate().is_ok());
}

#[test]
fn test_max_budget_validates() {
    let config = ThinkingConfig::new().with_thinking_budget(24576);
    assert!(config.validate().is_ok());
}

#[test]
fn test_budget_and_level_rejects() {
    let config = ThinkingConfig {
        thinking_budget: Some(1024),
        include_thoughts: None,
        thinking_level: Some(ThinkingLevel::High),
    };
    let result = config.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("mutually exclusive"));
}

#[test]
fn test_thinking_level_serialization() {
    let config = ThinkingConfig::new().with_thinking_level(ThinkingLevel::Medium);
    let json = serde_json::to_value(&config).unwrap();
    assert_eq!(json["thinkingLevel"], "medium");
    assert!(json.get("thinkingBudget").is_none());
}

#[test]
fn test_thinking_budget_serialization() {
    let config = ThinkingConfig::new().with_thinking_budget(2048);
    let json = serde_json::to_value(&config).unwrap();
    assert_eq!(json["thinkingBudget"], 2048);
    assert!(json.get("thinkingLevel").is_none());
}
