//! Live integration test for A2A client against running test agents.
//!
//! Prerequisites:
//! - Google ADK Agent running on port 8001 (check_prime, get_weather tools)
//! - LangGraph Agent running on port 8002 (general reasoning)
//!
//! Run with: cargo test -p adk-server --test a2a_live_integration_test -- --ignored --nocapture

use adk_server::a2a::{A2aClient, Message, Part, Role};

#[tokio::test]
#[ignore] // Requires running test agents
async fn test_resolve_agent_card_google_adk() {
    let card = A2aClient::resolve_agent_card("http://localhost:8001")
        .await
        .expect("Failed to resolve agent card from port 8001");

    println!("=== Google ADK Agent Card ===");
    println!("Name: {}", card.name);
    println!("URL: {}", card.url);
    println!("Skills: {:?}", card.skills.iter().map(|s| &s.name).collect::<Vec<_>>());

    assert_eq!(card.name, "helper_agent");
    assert!(!card.skills.is_empty());
}

#[tokio::test]
#[ignore] // Requires running test agents
async fn test_resolve_agent_card_langgraph() {
    let card = A2aClient::resolve_agent_card("http://localhost:8002")
        .await
        .expect("Failed to resolve agent card from port 8002");

    println!("=== LangGraph Agent Card ===");
    println!("Name: {}", card.name);
    println!("URL: {}", card.url);
    println!("Skills: {:?}", card.skills.iter().map(|s| &s.name).collect::<Vec<_>>());

    assert_eq!(card.name, "langgraph_gemini_agent");
    assert!(!card.skills.is_empty());
}

#[tokio::test]
#[ignore] // Requires running test agents
async fn test_message_send_check_prime() {
    let client = A2aClient::from_url("http://localhost:8001")
        .await
        .expect("Failed to create A2A client for port 8001");

    let message = Message::builder()
        .role(Role::User)
        .parts(vec![Part::text("Is 7 a prime number?".to_string())])
        .message_id(uuid::Uuid::new_v4().to_string())
        .build();

    let response = client.send_message(message).await.expect("Failed to send message");

    println!("=== message/send Response (check_prime) ===");
    println!("{}", serde_json::to_string_pretty(&response).unwrap());

    // Validate JSON-RPC response structure
    assert_eq!(response.jsonrpc, "2.0");
    assert!(response.error.is_none(), "Got error: {:?}", response.error);
    assert!(response.result.is_some(), "Missing result in response");

    let result = response.result.unwrap();
    // Should have task-like structure with id and status
    assert!(result.get("id").is_some(), "Response must have task 'id'");
    assert!(result.get("status").is_some(), "Response must have 'status'");

    let status = result.get("status").unwrap();
    let state = status.get("state").and_then(|s| s.as_str()).unwrap();
    // Accept both completed and failed (API key issues are external)
    assert!(
        state == "completed" || state == "failed",
        "Status state should be 'completed' or 'failed', got: {state}"
    );

    // If completed, check artifacts contain text about prime
    if state == "completed" {
        if let Some(artifacts) = result.get("artifacts").and_then(|a| a.as_array()) {
            let has_text = artifacts.iter().any(|art| {
                art.get("parts")
                    .and_then(|p| p.as_array())
                    .map(|parts| {
                        parts.iter().any(|part| {
                            part.get("text")
                                .and_then(|t| t.as_str())
                                .map(|t| t.to_lowercase().contains("prime") || t.contains("7"))
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            });
            assert!(has_text, "Expected artifact text about prime numbers");
        }
    } else {
        println!("⚠️  Agent returned 'failed' state (likely API key issue, not ADK-Rust bug)");
    }
}

#[tokio::test]
#[ignore] // Requires running test agents
async fn test_message_send_langgraph() {
    let client = A2aClient::from_url("http://localhost:8002")
        .await
        .expect("Failed to create A2A client for port 8002");

    let message = Message::builder()
        .role(Role::User)
        .parts(vec![Part::text("What is 3 + 5?".to_string())])
        .message_id(uuid::Uuid::new_v4().to_string())
        .build();

    let response = client.send_message(message).await.expect("Failed to send message");

    println!("=== message/send Response (LangGraph) ===");
    println!("{}", serde_json::to_string_pretty(&response).unwrap());

    // Validate JSON-RPC response structure
    assert_eq!(response.jsonrpc, "2.0");
    assert!(response.error.is_none(), "Got error: {:?}", response.error);
    assert!(response.result.is_some(), "Missing result in response");

    let result = response.result.unwrap();
    assert!(
        result.get("id").is_some() || result.get("status").is_some(),
        "Response missing task id or status"
    );
}

#[tokio::test]
#[ignore] // Requires running test agents
async fn test_a2a_protocol_compliance_response_format() {
    // Validates the response matches the expected A2A protocol format:
    // {
    //   "jsonrpc": "2.0",
    //   "id": "request-uuid",
    //   "result": {
    //     "id": "task-uuid",
    //     "contextId": "context-uuid",
    //     "status": {"state": "completed"|"failed"},
    //     "artifacts": [{"artifactId": "uuid", "parts": [...]}],
    //     "kind": "task"
    //   }
    // }

    // Use LangGraph agent (port 8002) for compliance test since it has a working API key
    let client =
        A2aClient::from_url("http://localhost:8002").await.expect("Failed to create A2A client");

    let message = Message::builder()
        .role(Role::User)
        .parts(vec![Part::text("What is 10 + 5?".to_string())])
        .message_id(uuid::Uuid::new_v4().to_string())
        .build();

    let response = client.send_message(message).await.expect("Failed to send message");

    println!("=== Protocol Compliance Check ===");
    println!("{}", serde_json::to_string_pretty(&response).unwrap());

    // 1. JSON-RPC 2.0 envelope
    assert_eq!(response.jsonrpc, "2.0", "Must be JSON-RPC 2.0");
    assert!(response.id.is_some(), "Must have request id");

    // 2. Result structure
    let result = response.result.expect("Must have result");

    // 3. Task ID
    let task_id = result.get("id").and_then(|v| v.as_str());
    assert!(task_id.is_some(), "Result must have task 'id' field");
    println!("Task ID: {}", task_id.unwrap());

    // 4. Status with state
    let status = result.get("status").expect("Result must have 'status' field");
    let state = status.get("state").and_then(|v| v.as_str());
    assert_eq!(state, Some("completed"), "Status state should be 'completed'");
    println!("Status: {:?}", state);

    // 5. History (LangGraph returns history instead of artifacts)
    let has_history = result.get("history").and_then(|v| v.as_array()).is_some();
    let has_artifacts = result.get("artifacts").and_then(|v| v.as_array()).is_some();
    assert!(has_history || has_artifacts, "Result must have 'history' or 'artifacts' array");

    // 6. Verify response contains the answer
    if let Some(history) = result.get("history").and_then(|v| v.as_array()) {
        let has_agent_response =
            history.iter().any(|msg| msg.get("role").and_then(|r| r.as_str()) == Some("agent"));
        assert!(has_agent_response, "History must contain an agent response");
        println!("✅ Agent response found in history");
    }

    if let Some(artifacts) = result.get("artifacts").and_then(|v| v.as_array()) {
        assert!(!artifacts.is_empty(), "Artifacts should not be empty");
        for (i, artifact) in artifacts.iter().enumerate() {
            let parts = artifact.get("parts").and_then(|v| v.as_array());
            assert!(parts.is_some(), "Artifact {} must have 'parts' array", i);
            println!("Artifact {}: {} parts", i, parts.unwrap().len());
        }
    }

    println!("\n✅ A2A Protocol Compliance: PASSED");
}
