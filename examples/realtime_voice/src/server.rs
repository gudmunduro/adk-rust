//! Web server for the "Mindfulness with Mia" realtime voice app.
//!
//! Architecture — **server-side bridge** (the ADK way):
//!
//! ```text
//!   browser  ──mic PCM16 (base64 over WS)──▶  Rust /ws handler
//!   browser  ◀──assistant PCM16 + events───   IntegratedRealtimeRunner
//!                                              │
//!                                              ├─ OpenAI Realtime (gpt-realtime)  OR
//!                                              │  Gemini Live (native audio)
//!                                              ├─ SessionService  (transcripts)
//!                                              ├─ MemoryService   (turn storage)
//!                                              └─ weather tool     (auto-executed)
//! ```
//!
//! The Rust server owns the realtime session through
//! [`IntegratedRealtimeRunner`], so transcript persistence, memory storage, and
//! tool execution all happen server-side — exactly what the integration layer
//! exists for. The browser is a thin audio device: it streams microphone PCM up
//! and plays the PCM the server streams back.
//!
//! The provider is chosen per session (browser `?provider=openai|gemini`). Their
//! audio rates differ — OpenAI is 24 kHz in/out, Gemini Live is 16 kHz in /
//! 24 kHz out — so the server negotiates the rates to the browser in a `ready`
//! message before any audio flows.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    Router,
    extract::Query,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::{Html, IntoResponse},
    routing::get,
};
use base64::Engine;
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

use adk_memory::InMemoryMemoryService;
use adk_realtime::config::{RealtimeConfig, VadConfig};
use adk_realtime::events::{ServerEvent, ToolCall};
use adk_realtime::gemini::{GeminiLiveBackend, GeminiRealtimeModel};
use adk_realtime::integration::{IntegratedRealtimeRunner, IntegrationConfig};
use adk_realtime::model::BoxedModel;
use adk_realtime::openai::OpenAIRealtimeModel;
use adk_realtime::runner::FnToolHandler;
use adk_session::{CreateRequest, InMemorySessionService, SessionService};

use crate::tools::get_weather_tool_def;

const APP_NAME: &str = "mindfulness-mia";
const USER_ID: &str = "shai";

const MIA_INSTRUCTION: &str = "You are Mia, a calm and empathetic mindfulness coach. \
You guide users through breathing exercises, meditation, and emotional awareness. \
Speak slowly, calmly, and thoughtfully. Address the user as Shai. \
Avoid somatic grounding exercises unless explicitly requested; favor breath \
awareness and cognitive reframing. Keep responses concise and soothing. \
If the user asks about the weather, use the get_weather tool.";

/// The realtime provider for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Provider {
    OpenAI,
    Gemini,
}

impl Provider {
    fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "gemini" | "google" => Provider::Gemini,
            _ => Provider::OpenAI,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Provider::OpenAI => "openai",
            Provider::Gemini => "gemini",
        }
    }

    /// (input_sample_rate, output_sample_rate) the browser must use.
    fn audio_rates(self) -> (u32, u32) {
        match self {
            // OpenAI GA Realtime is 24 kHz both ways.
            Provider::OpenAI => (24_000, 24_000),
            // Gemini Live consumes 16 kHz PCM16 and emits 24 kHz PCM16.
            Provider::Gemini => (16_000, 24_000),
        }
    }
}

/// Run the Axum web server.
pub async fn run_server(port: u16) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/", get(serve_index))
        .route("/ws", get(ws_handler))
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    info!("listening on 0.0.0.0:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Serve the embedded index.html.
async fn serve_index() -> impl IntoResponse {
    Html(include_str!("../assets/index.html"))
}

/// Messages the browser sends up the WebSocket.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClientMsg {
    /// A chunk of microphone audio (base64-encoded PCM16 mono).
    #[serde(rename = "input_audio")]
    InputAudio { audio: String },
    /// The user ended the session.
    #[serde(rename = "hangup")]
    Hangup,
}

/// Upgrade `/ws?provider=openai|gemini` to a per-connection realtime voice bridge.
async fn ws_handler(ws: WebSocketUpgrade, Query(params): Query<HashMap<String, String>>) -> impl IntoResponse {
    let provider = params.get("provider").map(|p| Provider::parse(p)).unwrap_or(Provider::OpenAI);
    ws.on_upgrade(move |socket| handle_voice_ws(socket, provider))
}

/// The weather tool — runs entirely server-side; its result is sent back to the
/// model automatically (auto_respond_tools) so Mia can speak it.
fn weather_tool() -> FnToolHandler<impl Fn(&ToolCall) -> adk_realtime::error::Result<serde_json::Value> + Send + Sync>
{
    FnToolHandler::new(|call: &ToolCall| {
        let city = call.arguments.get("city").and_then(|v| v.as_str()).unwrap_or("your area");
        info!(city = %city, "🔧 weather tool executed");
        Ok(json!({
            "city": city,
            "temperature_f": 68,
            "conditions": "clear skies",
            "summary": format!("It's a calm, clear 68°F in {city}."),
        }))
    })
}

/// Build the provider-specific realtime model.
fn build_model(provider: Provider) -> anyhow::Result<(BoxedModel, &'static str)> {
    match provider {
        Provider::OpenAI => {
            let api_key = std::env::var("OPENAI_API_KEY")
                .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY is not set"))?;
            let model_id = std::env::var("OPENAI_REALTIME_MODEL")
                .unwrap_or_else(|_| "gpt-realtime".to_string());
            let model: BoxedModel = Arc::new(OpenAIRealtimeModel::new(api_key, model_id));
            Ok((model, "marin")) // marin: a natural GA voice
        }
        Provider::Gemini => {
            let api_key = std::env::var("GEMINI_API_KEY")
                .or_else(|_| std::env::var("GOOGLE_API_KEY"))
                .map_err(|_| anyhow::anyhow!("GEMINI_API_KEY / GOOGLE_API_KEY is not set"))?;
            // AI Studio (API-key) endpoint uses different model names than the
            // Agent Platform/Vertex endpoint (the crate's default
            // `models/gemini-live-2.5-flash-native-audio` is the *Vertex* name and
            // 404s here). We default to the half-cascade live model, which calls
            // tools far more reliably than the native-audio model — important for
            // this tool-using agent. For the most natural voice (but weaker tool
            // use), set GEMINI_REALTIME_MODEL=models/gemini-2.5-flash-native-audio-preview-12-2025.
            let model_id = std::env::var("GEMINI_REALTIME_MODEL")
                .unwrap_or_else(|_| "models/gemini-3.1-flash-live-preview".to_string());
            let model: BoxedModel =
                Arc::new(GeminiRealtimeModel::new(GeminiLiveBackend::studio(api_key), model_id));
            Ok((model, "Kore")) // Kore: a Gemini Live voice
        }
    }
}

/// Build an [`IntegratedRealtimeRunner`] for one browser session, wiring the
/// chosen provider to in-memory session + memory services and the weather tool.
async fn build_runner(provider: Provider, session_id: &str) -> anyhow::Result<IntegratedRealtimeRunner> {
    let (model, voice) = build_model(provider)?;

    // Server VAD lets the model decide turn boundaries and auto-respond — no
    // explicit create_response needed after each user utterance.
    let config = RealtimeConfig::default()
        .with_instruction(MIA_INSTRUCTION)
        .with_voice(voice)
        .with_audio_only()
        .with_vad(VadConfig::server_vad())
        .with_transcription();

    let session_service = Arc::new(InMemorySessionService::new());
    let memory_service = Arc::new(InMemoryMemoryService::new());

    // Create the session up front so transcript persistence has a home.
    session_service
        .create(CreateRequest {
            app_name: APP_NAME.to_string(),
            user_id: USER_ID.to_string(),
            session_id: Some(session_id.to_string()),
            state: Default::default(),
        })
        .await
        .map_err(|e| anyhow::anyhow!("session create failed: {e}"))?;

    let runner = IntegratedRealtimeRunner::builder()
        .model(model)
        .config(config)
        .identity(APP_NAME, USER_ID, session_id)
        .session_service(session_service)
        .memory_service(memory_service)
        .integration_config(IntegrationConfig::default())
        .tool(get_weather_tool_def(), weather_tool())
        .build()?;

    Ok(runner)
}

/// Headless smoke test of the full integration path (no browser/mic needed).
///
/// Connects via [`IntegratedRealtimeRunner`], asks Mia a weather question by
/// text, and pumps events — reporting whether the tool executed, how much audio
/// came back, and the transcript. Run with `cargo run -- probe [openai|gemini]`.
pub async fn run_probe(provider: &str) -> anyhow::Result<()> {
    let provider = Provider::parse(provider);
    info!(provider = provider.name(), "probe: starting");
    let session_id = uuid::Uuid::new_v4().to_string();
    let runner = build_runner(provider, &session_id).await?;
    runner.connect().await?;
    info!("probe: connected; sending a weather question by text");

    runner.send_text("What's the weather in Seattle right now? Answer in one short sentence.").await?;
    runner.create_response().await?;

    let mut audio_bytes = 0usize;
    let mut transcript = String::new();
    let mut thinking = String::new();
    let mut tool_seen = false;

    loop {
        let next = tokio::time::timeout(std::time::Duration::from_secs(25), runner.next_event()).await;
        let event = match next {
            Ok(Some(Ok(ev))) => ev,
            Ok(Some(Err(e))) => {
                warn!(error = %e, "probe: stream error");
                break;
            }
            Ok(None) => break,
            Err(_) => {
                warn!("probe: timed out waiting for events");
                break;
            }
        };
        match event {
            ServerEvent::AudioDelta { delta, .. } => audio_bytes += delta.len(),
            // Spoken-answer transcript (OpenAI audio transcript / Gemini outputTranscription).
            ServerEvent::TranscriptDelta { delta, .. } => transcript.push_str(&delta),
            // Gemini "thinking" text (modelTurn text parts) — tracked separately.
            ServerEvent::TextDelta { delta, .. } => thinking.push_str(&delta),
            ServerEvent::FunctionCallDone { name, .. } => {
                info!(tool = %name, "probe: model requested a tool call");
                tool_seen = true;
            }
            ServerEvent::ResponseDone { .. } => {
                // A function call produces a first response that ends here; the
                // spoken answer arrives in a second response after the tool runs.
                if tool_seen && transcript.is_empty() && audio_bytes == 0 {
                    continue;
                }
                break;
            }
            ServerEvent::Error { error, .. } => {
                anyhow::bail!("realtime error: {}", error.message);
            }
            _ => {}
        }
    }

    runner.close().await.ok();
    info!(
        provider = provider.name(),
        tool_call_seen = tool_seen,
        audio_bytes,
        transcript = %transcript,
        thinking = %thinking,
        "probe: complete"
    );
    anyhow::ensure!(
        audio_bytes > 0 || !transcript.is_empty(),
        "no assistant output received"
    );
    Ok(())
}

/// Drive one realtime voice session: pump mic audio up, stream events/audio down.
async fn handle_voice_ws(socket: WebSocket, provider: Provider) {
    let session_id = uuid::Uuid::new_v4().to_string();
    info!(session_id = %session_id, provider = provider.name(), "voice session starting");

    let (mut sender, mut receiver) = socket.split();

    // Build + connect the integrated runner; report failures to the browser.
    let runner = match build_runner(provider, &session_id).await {
        Ok(r) => Arc::new(r),
        Err(e) => {
            warn!(error = %e, "failed to build runner");
            let _ = sender
                .send(Message::Text(
                    json!({"type":"error","message": e.to_string()}).to_string().into(),
                ))
                .await;
            return;
        }
    };
    if let Err(e) = runner.connect().await {
        warn!(error = %e, "failed to connect realtime session");
        let _ = sender
            .send(Message::Text(
                json!({"type":"error","message": format!("connect failed: {e}")})
                    .to_string()
                    .into(),
            ))
            .await;
        return;
    }

    // Tell the browser which sample rates to use before any audio flows.
    let (input_rate, output_rate) = provider.audio_rates();
    let ready = json!({
        "type": "ready",
        "provider": provider.name(),
        "input_rate": input_rate,
        "output_rate": output_rate,
    });
    if sender.send(Message::Text(ready.to_string().into())).await.is_err() {
        return;
    }
    info!(session_id = %session_id, provider = provider.name(), "realtime session connected");

    // Outbound: realtime events → browser. Owns the WS sink.
    let out_runner = runner.clone();
    let outbound = async move {
        while let Some(event) = out_runner.next_event().await {
            let msg = match event {
                Ok(ev) => server_event_to_client_json(ev),
                Err(e) => Some(json!({"type":"error","message": e.to_string()})),
            };
            if let Some(payload) = msg
                && sender.send(Message::Text(payload.to_string().into())).await.is_err()
            {
                break; // browser went away
            }
        }
    };

    // Inbound: browser mic audio → realtime session.
    let in_runner = runner.clone();
    let inbound = async move {
        while let Some(Ok(frame)) = receiver.next().await {
            match frame {
                Message::Text(text) => match serde_json::from_str::<ClientMsg>(&text) {
                    Ok(ClientMsg::InputAudio { audio }) => {
                        if let Err(e) = in_runner.send_audio(&audio).await {
                            warn!(error = %e, "send_audio failed");
                            break;
                        }
                    }
                    Ok(ClientMsg::Hangup) => break,
                    Err(_) => {} // ignore unknown control frames
                },
                Message::Close(_) => break,
                _ => {}
            }
        }
    };

    // Whichever side ends first tears down the session.
    tokio::select! {
        _ = outbound => info!(session_id = %session_id, "realtime stream ended"),
        _ = inbound => info!(session_id = %session_id, "browser disconnected"),
    }

    let _ = runner.close().await;
    info!(session_id = %session_id, "voice session closed");
}

/// Translate a realtime [`ServerEvent`] into the compact JSON the browser UI
/// consumes. Returns `None` for events the UI doesn't render.
fn server_event_to_client_json(event: ServerEvent) -> Option<serde_json::Value> {
    match event {
        ServerEvent::AudioDelta { delta, .. } => {
            // delta is decoded PCM16 bytes; re-encode for the browser to play.
            let audio = base64::engine::general_purpose::STANDARD.encode(&delta);
            Some(json!({ "type": "audio", "audio": audio }))
        }
        // The assistant's spoken words: OpenAI and Gemini both surface them as a
        // transcript delta (Gemini via outputAudioTranscription). Gemini's
        // separate `TextDelta` carries model "thinking" — intentionally not shown.
        ServerEvent::TranscriptDelta { delta, .. } => {
            Some(json!({ "type": "assistant_transcript", "delta": delta }))
        }
        // User speech transcription: OpenAI sends a single completed event;
        // Gemini streams deltas.
        ServerEvent::InputTranscriptDelta { delta, .. } => {
            Some(json!({ "type": "user_transcript_delta", "delta": delta }))
        }
        ServerEvent::InputTranscriptCompleted { transcript, .. } => {
            Some(json!({ "type": "user_transcript", "text": transcript }))
        }
        ServerEvent::SpeechStarted { .. } => Some(json!({ "type": "user_speaking" })),
        ServerEvent::SpeechStopped { .. } => Some(json!({ "type": "user_stopped" })),
        ServerEvent::ResponseDone { .. } => Some(json!({ "type": "response_done" })),
        ServerEvent::FunctionCallDone { name, arguments, .. } => Some(json!({
            "type": "decision",
            "text": format!("Mia is calling {name}({arguments})"),
        })),
        ServerEvent::Error { error, .. } => Some(json!({ "type": "error", "message": error.message })),
        _ => None,
    }
}
