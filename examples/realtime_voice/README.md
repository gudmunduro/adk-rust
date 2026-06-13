# Realtime Voice — Mindfulness with Mia

A full **web UI** demonstrating ADK-Rust real-time voice — a mindfulness coaching
app where the **Rust server owns the realtime session** through
[`IntegratedRealtimeRunner`], so transcripts, memory, and tool execution all
happen server-side. The browser is a thin audio device.

## What This Shows

| Capability | Description |
|-----------|-------------|
| **Full Web UI** | Browser-based voice coaching interface served by Axum |
| **Server-side bridge** | The Rust server owns the realtime session via `IntegratedRealtimeRunner`; the browser only captures/plays audio |
| **Audio capture** | Web Audio API microphone capture at 24 kHz PCM16, streamed to the server over WebSocket |
| **Audio playback** | Gapless Web Audio playback of the assistant's PCM stream, with barge-in |
| **Session persistence** | Completed turns persisted to a `SessionService` |
| **Memory** | Turns stored to a `MemoryService` for future recall |
| **Tool calling** | `get_weather` executed **server-side**; the result is fed back to the model |
| **VAD** | Server-side voice activity detection for natural turn-taking |
| **Coaching persona** | "Mia" mindfulness coach with guidelines and preferences |

## Architecture

```text
┌─────────────────────────────────────────────────────────────┐
│                      Browser (Web UI)                         │
│   mic ──PCM16 (base64 over WS)──▶        ◀── assistant PCM16  │
│   Web Audio capture @ 24 kHz             Web Audio playback   │
└───────────┬───────────────────────────────────────▲──────────┘
            │ WebSocket /ws                          │
┌───────────▼───────────────────────────────────────┴──────────┐
│     Axum server (localhost:3033)                              │
│  ┌──────────────────────────────────────────────────────┐    │
│  │  IntegratedRealtimeRunner                            │    │
│  │  ├─ OpenAIRealtimeModel  (gpt-realtime, voice "marin") │    │
│  │  ├─ SessionService  → transcript persistence          │    │
│  │  ├─ MemoryService   → turn storage / recall           │    │
│  │  └─ get_weather tool → auto-executed, result returned │    │
│  └──────────────────────────────────────────────────────┘    │
└──────────────────────────────────────────────────────────────┘
```

The server never exposes your API key to the browser — the realtime connection
to OpenAI lives entirely on the server side.

## Providers

Pick **OpenAI** or **Gemini** from the dropdown before starting a session — the
browser passes the choice to the server (`/ws?provider=…`), which builds the
matching realtime model. Because their audio rates differ (OpenAI 24 kHz in/out;
Gemini Live 16 kHz in / 24 kHz out), the server negotiates the sample rates to
the browser in a `ready` message before any audio flows, and the browser
configures its capture/playback contexts accordingly.

## Prerequisites

- Rust 1.94.0+
- `OPENAI_API_KEY` (for OpenAI) and/or `GEMINI_API_KEY` / `GOOGLE_API_KEY` (for Gemini)
- A modern browser with WebSocket + Web Audio API support and microphone access

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `OPENAI_API_KEY` | For OpenAI | OpenAI API key |
| `GEMINI_API_KEY` / `GOOGLE_API_KEY` | For Gemini | Google AI Studio key |
| `OPENAI_REALTIME_MODEL` | No | OpenAI model ID (default: `gpt-realtime`; `gpt-realtime-2` for the reasoning model) |
| `GEMINI_REALTIME_MODEL` | No | Gemini model ID (default: `models/gemini-3.1-flash-live-preview`, which calls tools reliably; `models/gemini-2.5-flash-native-audio-preview-12-2025` for the most natural voice) |
| `PORT` | No | Server port (default: `3033`) |
| `RUST_LOG` | No | Log level (default: `info`) |

> **Gemini model note:** AI Studio (API-key) uses different model names than
> Vertex/Agent Platform. The half-cascade `gemini-3.1-flash-live-preview` is the
> default here because the native-audio model, while more natural-sounding, calls
> tools far less reliably.

## Run

```bash
# Web UI
cargo run --manifest-path examples/realtime_voice/Cargo.toml
# → open http://localhost:3033

# Headless smoke test (no browser/mic) — connects, asks a weather question by
# text, verifies the tool runs and audio comes back. Pick a provider:
cargo run --manifest-path examples/realtime_voice/Cargo.toml -- probe openai
cargo run --manifest-path examples/realtime_voice/Cargo.toml -- probe gemini
```

## How It Works

1. Click **START VOICE SESSION** — the browser opens a WebSocket to the server.
2. The server builds an `IntegratedRealtimeRunner` (OpenAI model + in-memory
   `SessionService`/`MemoryService` + the `get_weather` tool) and connects.
3. The browser captures microphone audio as 24 kHz PCM16 and streams base64
   frames up the WebSocket.
4. Server VAD detects turn boundaries; the model responds automatically.
5. The runner streams the assistant's PCM audio + transcript back to the browser,
   which plays it gaplessly via Web Audio. Barge-in flushes playback.
6. When the model calls `get_weather`, the runner executes it **server-side** and
   returns the result so Mia can speak it.
7. Each completed turn is persisted to the session and stored to memory.

## UI

- **Left panel** — avatar, voice controls (start / mute / pause / hang up),
  loaded memory contexts, coaching guidelines, MIA/USER status.
- **Right panel** — User Memory Insights, Coaching Strategy, and a live
  Pipeline Decisions log (tool calls and session events).

## Feature Flags

```toml
adk-realtime = { version = "1.1.0", features = ["openai", "gemini", "integration"] }
```
