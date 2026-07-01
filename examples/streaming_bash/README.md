# streaming_bash

Live tool output in the browser — an `LlmAgent` whose tool activity (calls,
streaming stdout/stderr, and final results) is rendered from a **single
`EventStream`**.

This example demonstrates two `adk-core` capabilities:

1. **Streaming tool progress** — `ToolContext::emit_progress` lets a tool push
   stdout/stderr to the UI *while it is still running*. `BashTool`
   (`adk-devtools`) streams each line; the framework forwards it as a partial
   `Event`.
2. **First-class tool results** — `Event::tool_calls()` and
   `Event::tool_results()` surface every tool's call and output generically, so
   the UI renders **any** tool — streaming (`bash`) or one-shot (`read_file`,
   `grep`, `glob`) — not just shell tools.

## Architecture

```text
  browser ──prompt (JSON over WS)──▶ Rust /ws handler
  browser ◀──model text + tool ─────  LlmAgent + DevTools (Gemini)
           calls/progress/results      ├─ bash      → streams stdout/stderr
           (events)                     ├─ read_file → one-shot result
                                        ├─ grep      → one-shot result
                                        └─ glob      → one-shot result
```

The server owns the agent. For each `Event` it forwards a typed frame derived
from first-class accessors:

| Accessor | Frame | UI effect |
|----------|-------|-----------|
| `event.tool_calls()` | `tool_call` | open a card (tool name + args) |
| `event.tool_progress_stream()` | `terminal` | append a live stdout/stderr line |
| `event.tool_results()` | `tool_result` | finalize the card with the output |
| text parts | `token` | append to the assistant's reply |

Cards are correlated by `call_id`, so streaming and one-shot tools both render
correctly even across multiple tool calls in one turn.

## Run

```bash
# Web UI (default) — open http://localhost:3000
cargo run --manifest-path examples/streaming_bash/Cargo.toml

# Console demo
cargo run --manifest-path examples/streaming_bash/Cargo.toml -- cli
```

Set `PORT` to change the web port (e.g. `PORT=3717 cargo run ...`).

## Configuration

| Variable | Required | Description |
|----------|----------|-------------|
| `GOOGLE_API_KEY` | yes | Gemini API key (model: `gemini-2.5-flash`). |
| `PORT` | no | Web server port (default `3000`). |

## Try

- `Run exactly: for i in $(seq 1 10); do echo "tick $i"; sleep 0.4; done` — watch
  lines stream in live (one every 0.4s) via `bash`.
- `Read the root Cargo.toml` — `read_file` renders its result with no streaming.
- `Search for "tokio" in the root Cargo.toml` — `grep` result card.
- `Find all README.md files in examples` — `glob` result card.

## Key code

- `src/server.rs` — `run_turn` translates each `Event` into a typed WS frame
  using `tool_calls()` / `tool_progress_stream()` / `tool_results()`.
- `assets/index.html` — renders one card per tool call (keyed by `call_id`),
  streaming `bash` output live and finalizing every tool with its result.
- The streaming `bash` tool itself lives in `adk-devtools`
  (`src/tools/bash.rs`), and `emit_progress` is defined on `ToolContext` in
  `adk-core`.
