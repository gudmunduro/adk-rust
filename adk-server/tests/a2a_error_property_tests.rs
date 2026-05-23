//! Property-based tests for A2A error message clarity.
//!
//! **Feature: a2a-simple-scaffolding, Property 6: Error Message Clarity**
//!
//! *For any* invalid configuration passed to the A2aServer builder (missing agent,
//! agent without name), the error message SHALL contain actionable guidance
//! identifying the specific issue.
//!
//! **Validates: Requirements 9.1, 9.2, 9.3**

#![cfg(feature = "a2a-v1")]

use std::sync::Arc;

use adk_core::{Agent, EventStream, InvocationContext, Result as AdkResult};
use adk_server::A2aServer;
use async_trait::async_trait;
use futures::stream;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Test agent with configurable name
// ---------------------------------------------------------------------------

struct ErrorTestAgent {
    name: String,
    description: String,
}

impl ErrorTestAgent {
    fn new(name: String, description: String) -> Self {
        Self { name, description }
    }
}

#[async_trait]
impl Agent for ErrorTestAgent {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn sub_agents(&self) -> &[Arc<dyn Agent>] {
        &[]
    }

    async fn run(&self, _ctx: Arc<dyn InvocationContext>) -> AdkResult<EventStream> {
        Ok(Box::pin(stream::empty()))
    }
}

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

/// Generate empty or whitespace-only strings for agent names.
fn arb_empty_name() -> impl Strategy<Value = String> {
    prop_oneof![Just(String::new()), " {0,10}".prop_map(|s| s.to_string()),]
        .prop_filter("must be effectively empty", |s| s.trim().is_empty())
}

/// Generate non-empty description strings.
fn arb_description() -> impl Strategy<Value = String> {
    "[a-zA-Z ]{1,50}"
}

// ---------------------------------------------------------------------------
// Property tests
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Missing agent: error must mention "agent" and provide actionable guidance.
    #[test]
    fn prop_missing_agent_error_clarity(
        bind_addr in prop_oneof![
            Just("0.0.0.0:8080".to_string()),
            Just("127.0.0.1:9090".to_string()),
            Just("localhost:3000".to_string()),
        ],
    ) {
        let result = A2aServer::builder()
            .bind_addr(&bind_addr)
            .build();

        prop_assert!(result.is_err(), "Builder should fail without an agent");

        let err = result.unwrap_err();
        let msg = &err.message;

        // Error must mention "agent" to identify the issue
        prop_assert!(
            msg.to_lowercase().contains("agent"),
            "Error message must mention 'agent', got: {msg}"
        );

        // Error must contain actionable guidance (how to fix it)
        prop_assert!(
            msg.contains(".agent(") || msg.contains("Call"),
            "Error message must contain actionable guidance, got: {msg}"
        );
    }

    /// Empty agent name: error must mention "name" and provide actionable guidance.
    #[test]
    fn prop_empty_agent_name_error_clarity(
        name in arb_empty_name(),
        description in arb_description(),
    ) {
        // The agent trait returns the name as-is; only truly empty names trigger the error
        // (whitespace names are not trimmed by the builder)
        let agent: Arc<dyn Agent> = Arc::new(ErrorTestAgent::new(
            name.clone(),
            description,
        ));

        let result = A2aServer::builder()
            .agent(agent)
            .build();

        // Only empty string triggers the error (whitespace is not trimmed)
        if name.is_empty() {
            prop_assert!(result.is_err(), "Builder should fail with empty agent name");

            let err = result.unwrap_err();
            let msg = &err.message;

            // Error must mention "name" to identify the issue
            prop_assert!(
                msg.to_lowercase().contains("name"),
                "Error message must mention 'name', got: {msg}"
            );

            // Error must contain actionable guidance
            prop_assert!(
                msg.contains("LlmAgentBuilder") || msg.contains("non-empty"),
                "Error message must contain actionable guidance, got: {msg}"
            );
        }
    }
}
