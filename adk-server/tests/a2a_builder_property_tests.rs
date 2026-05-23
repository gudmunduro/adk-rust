//! Property-based tests for A2A builder configuration composition.
//!
//! **Feature: a2a-simple-scaffolding, Property 3: Builder Configuration Composition**
//!
//! *For any* valid combination of builder options (non-empty bind address, any
//! session service, any agent card metadata strings), the builder SHALL produce
//! a valid `A2aServerApp` without error when an agent is provided.
//!
//! **Validates: Requirements 6.1, 6.2, 6.3, 6.4, 6.5**

#![cfg(feature = "a2a-v1")]

use std::sync::Arc;

use adk_core::{Agent, EventStream, InvocationContext, Result as AdkResult};
use adk_server::A2aServer;
use adk_session::InMemorySessionService;
use async_trait::async_trait;
use futures::stream;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Test agent
// ---------------------------------------------------------------------------

struct PropTestAgent {
    name: String,
    description: String,
}

impl PropTestAgent {
    fn new(name: String, description: String) -> Self {
        Self { name, description }
    }
}

#[async_trait]
impl Agent for PropTestAgent {
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

/// Generate valid bind addresses in the format "host:port".
fn arb_bind_addr() -> impl Strategy<Value = String> {
    (
        prop_oneof![
            Just("0.0.0.0".to_string()),
            Just("127.0.0.1".to_string()),
            Just("localhost".to_string()),
        ],
        1024u16..65535,
    )
        .prop_map(|(host, port)| format!("{host}:{port}"))
}

/// Generate non-empty agent names (valid identifiers).
fn arb_agent_name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9-]{0,30}".prop_filter("must not end with hyphen", |s| !s.ends_with('-'))
}

/// Generate agent card metadata strings (non-empty).
fn arb_metadata_string() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 _.-]{1,50}"
}

/// Generate version strings.
fn arb_version() -> impl Strategy<Value = String> {
    (1u32..10, 0u32..20, 0u32..100)
        .prop_map(|(major, minor, patch)| format!("{major}.{minor}.{patch}"))
}

/// Generate URL strings.
fn arb_url() -> impl Strategy<Value = String> {
    (
        prop_oneof![Just("http"), Just("https")],
        "[a-z]{3,12}",
        prop_oneof![Just("com"), Just("io"), Just("dev")],
    )
        .prop_map(|(scheme, domain, tld)| format!("{scheme}://{domain}.{tld}"))
}

// ---------------------------------------------------------------------------
// Property test
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_builder_configuration_composition(
        bind_addr in arb_bind_addr(),
        agent_name in arb_agent_name(),
        agent_desc in arb_metadata_string(),
        card_name in arb_metadata_string(),
        card_desc in arb_metadata_string(),
        card_version in arb_version(),
        card_url in arb_url(),
        streaming in proptest::bool::ANY,
        push_notifications in proptest::bool::ANY,
        use_custom_session in proptest::bool::ANY,
    ) {
        let agent: Arc<dyn Agent> = Arc::new(PropTestAgent::new(
            agent_name.clone(),
            agent_desc,
        ));

        let mut builder = A2aServer::builder()
            .agent(agent)
            .bind_addr(&bind_addr)
            .agent_card_name(&card_name)
            .agent_card_description(&card_desc)
            .agent_card_version(&card_version)
            .agent_card_url(&card_url)
            .streaming(streaming)
            .push_notifications(push_notifications);

        if use_custom_session {
            let session_service: Arc<dyn adk_session::SessionService> =
                Arc::new(InMemorySessionService::new());
            builder = builder.session_service(session_service);
        }

        let result = builder.build();
        prop_assert!(
            result.is_ok(),
            "Builder failed for bind_addr={bind_addr}, agent_name={agent_name}: {:?}",
            result.err()
        );

        let app = result.unwrap();
        prop_assert_eq!(app.bind_addr(), &bind_addr);
    }
}
