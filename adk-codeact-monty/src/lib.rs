//! A Python [`CodeRuntime`] for the ADK-Rust [`CodeAgent`], backed by
//! [Pydantic Monty](https://github.com/pydantic/monty) — a minimal, secure,
//! Rust-native Python interpreter built for running LLM-generated code.
//!
//! [`MontyRuntime`] lets a `CodeAgent` *act by writing Python*: the model emits a
//! script each turn, invokes your [`Tool`](adk_core::Tool)s with the built-in
//! `call_tool("name", {"arg": value})` function, composes their results with real
//! control flow, and returns a tagged value. Monty executes that script
//! in-process in microseconds, with no
//! container, no subprocess, and no filesystem/network access — and can snapshot
//! a paused run to bytes, which is exactly what the CodeAct suspend/resume model
//! (HITL confirmation, long-running tools, durable checkpoints) requires.
//!
//! # Quick start
//!
//! ```no_run
//! use std::sync::Arc;
//! use adk_agent::codeact::CodeAgent;
//! use adk_codeact_monty::MontyRuntime;
//! # use adk_core::Llm;
//! # fn wire(model: Arc<dyn Llm>) -> Result<(), Box<dyn std::error::Error>> {
//! let agent = CodeAgent::builder()
//!     .name("python_agent")
//!     .model(model)
//!     .runtime(Arc::new(MontyRuntime::new()))
//!     .instruction("Solve the task by writing Python.")
//!     // .tool(Arc::new(MyTool))
//!     .build()?;
//! # let _ = agent;
//! # Ok(())
//! # }
//! ```
//!
//! # Resource limits
//!
//! Cap a script's time, memory, or allocations with the builder — limits ride
//! along inside a serialized continuation, so a resumed run stays bounded:
//!
//! ```
//! use std::time::Duration;
//! use adk_codeact_monty::MontyRuntime;
//!
//! let runtime = MontyRuntime::builder()
//!     .max_duration(Duration::from_secs(2))
//!     .max_memory(64 * 1024 * 1024)
//!     .build();
//! # let _ = runtime;
//! ```
//!
//! [`CodeAgent`]: adk_agent::codeact::CodeAgent
//! [`CodeRuntime`]: adk_agent::codeact::CodeRuntime

#![warn(missing_docs)]

mod convert;
mod prompt;
mod runtime;

pub use runtime::{MontyRuntime, MontyRuntimeBuilder};

/// Re-export of Monty's resource-limit configuration, for
/// [`MontyRuntimeBuilder::resource_limits`].
pub use monty::ResourceLimits;
