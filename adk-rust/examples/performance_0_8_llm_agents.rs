//! Compile-checked, keyless examples for the v0.8.0 performance work.
//!
//! Each case builds and runs an LLM agent with `MockLlm`, so the examples
//! validate the agent-facing ergonomics without requiring external API keys.

use adk_rust::model::MockLlm;
use adk_rust::prelude::*;
use adk_rust::session::{CreateRequest, InMemorySessionService, SessionService};
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;

struct UseCase {
    finding: &'static str,
    agent_name: &'static str,
    user_prompt: &'static str,
    validated_response: &'static str,
    run_config: RunConfig,
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut cases = vec![
        UseCase {
            finding: "1. current cargo-adk scaffolds",
            agent_name: "scaffold_advisor",
            user_prompt: "Create a small support bot project I can compile quickly.",
            validated_response: "Use cargo-adk's 0.8 templates with minimal features for the first build.",
            run_config: RunConfig::default(),
        },
        UseCase {
            finding: "2. rustls-only HTTP clients",
            agent_name: "install_doctor",
            user_prompt: "My Linux install is failing around OpenSSL. What should I try?",
            validated_response: "Use the rustls-backed dependency set; native-tls is no longer part of the starter path.",
            run_config: RunConfig::default(),
        },
        UseCase {
            finding: "3. true minimal starter tier",
            agent_name: "starter_agent",
            user_prompt: "I need a simple appointment reminder agent with the smallest build.",
            validated_response: "Start with the minimal tier: agent, Gemini model, runner, and sessions.",
            run_config: RunConfig::default(),
        },
        UseCase {
            finding: "4. opt-in CLI providers",
            agent_name: "cli_provider_advisor",
            user_prompt: "I only use Gemini in the CLI. Do I need every provider compiled?",
            validated_response: "No. Install the CLI with only the provider features you need.",
            run_config: RunConfig::default(),
        },
        UseCase {
            finding: "5. telemetry core without OTLP",
            agent_name: "telemetry_triage",
            user_prompt: "I want logs locally now and can add OTLP later.",
            validated_response: "Use telemetry core for tracing now, then enable telemetry-otlp when exporting to a collector.",
            run_config: RunConfig::default(),
        },
        UseCase {
            finding: "6. MCP tools are opt-in",
            agent_name: "tooling_advisor",
            user_prompt: "My agent only calls local Rust function tools.",
            validated_response: "Keep MCP disabled until you connect an MCP server.",
            run_config: RunConfig::default(),
        },
        UseCase {
            finding: "7. Gemini backtraces are debug-only",
            agent_name: "gemini_debug_advisor",
            user_prompt: "How do I keep release builds lean but preserve deep errors in debug builds?",
            validated_response: "Use the default lean Gemini client and enable the backtrace feature only for debugging.",
            run_config: RunConfig::default(),
        },
        UseCase {
            finding: "8. empty state-delta session fast path",
            agent_name: "session_budgeter",
            user_prompt: "Most of my turns do not change state. Can persistence be cheaper?",
            validated_response: "Empty state deltas now skip state merge work and only append the event.",
            run_config: RunConfig::default(),
        },
        UseCase {
            finding: "9. bounded history loading",
            agent_name: "history_window_support",
            user_prompt: "Load only the recent turns for a support chat handoff.",
            validated_response: "Set RunConfig.history_max_events to bound startup work while preserving recent context.",
            run_config: RunConfig { history_max_events: Some(12), ..RunConfig::default() },
        },
        UseCase {
            finding: "10. payload-safe tracing",
            agent_name: "privacy_observer",
            user_prompt: "Trace enough to debug without leaking long customer payloads.",
            validated_response: "Keep record_payloads off and use trace_payload_max_bytes to cap recorded payload fields.",
            run_config: RunConfig { trace_payload_max_bytes: 256, ..RunConfig::default() },
        },
        UseCase {
            finding: "11. bounded parallel tools",
            agent_name: "operations_dispatcher",
            user_prompt: "Call several read-only inventory tools without flooding the backend.",
            validated_response: "Set RunConfig.max_tool_concurrency to cap parallel tool execution.",
            run_config: RunConfig { max_tool_concurrency: Some(4), ..RunConfig::default() },
        },
        UseCase {
            finding: "12. cache lifecycle without mutex-held network waits",
            agent_name: "cache_operator",
            user_prompt: "Refresh prompt caches without blocking other sessions.",
            validated_response: "Cache create/delete calls now run outside the cache manager mutex.",
            run_config: RunConfig::default(),
        },
    ];

    for case in cases.drain(..) {
        let output = run_case(case).await?;
        println!("{output}");
    }

    Ok(())
}

async fn run_case(case: UseCase) -> Result<String> {
    let model = MockLlm::new(format!("{}_mock", case.agent_name))
        .with_response(LlmResponse::new(Content::new("model").with_text(case.validated_response)));

    let agent = Arc::new(
        LlmAgentBuilder::new(case.agent_name)
            .description(format!("Validation agent for {}", case.finding))
            .instruction("Answer with one practical recommendation for the user's adoption task.")
            .model(Arc::new(model))
            .build()?,
    );

    let session_service: Arc<dyn SessionService> = Arc::new(InMemorySessionService::new());
    let session_id = format!("session_{}", case.agent_name);
    session_service
        .create(CreateRequest {
            app_name: "performance_0_8_examples".to_string(),
            user_id: "user".to_string(),
            session_id: Some(session_id.clone()),
            state: HashMap::new(),
        })
        .await?;

    let runner = Runner::builder()
        .app_name("performance_0_8_examples")
        .agent(agent)
        .session_service(session_service)
        .run_config(case.run_config)
        .build()?;

    let mut stream = runner
        .run_str("user", &session_id, Content::new("user").with_text(case.user_prompt))
        .await?;

    let mut text = String::new();
    while let Some(event) = stream.next().await {
        let event = event?;
        if let Some(content) = event.llm_response.content {
            for part in content.parts {
                if let Some(part_text) = part.text() {
                    text.push_str(part_text);
                }
            }
        }
    }

    Ok(format!("{}: {}", case.finding, text))
}
