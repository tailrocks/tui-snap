//! MCP-ish JSON-RPC 2.0 stdio server over the typed op protocol (A08).
//!
//! One MCP tool per [`crate::proto::Op`] variant; `tools/call` executes via
//! [`crate::proto::execute`] and returns the [`crate::proto::OpResult`] /
//! [`crate::proto::OpError`] verbatim inside MCP content.
//!
//! Transport: newline-delimited JSON-RPC 2.0 on stdio, hand-rolled on
//! `serde_json` only (no new dependencies).
//!
//! Composition note: there is intentionally no `tuiscotti mcp` subcommand here
//! (`main.rs` is owned by another agent). Agents either link this module
//! ([`run_stdio`]) or drive the equivalent vocabulary through the already
//! shipped `tuiscotti --machine` JSON-lines protocol, which speaks the same
//! [`crate::proto::Op`] / envelope types. Tests drive [`serve`] over piped
//! buffers.
//!
//! Methods:
//! - `initialize` → `{protocolVersion, capabilities: {tools: {}}, serverInfo}`
//! - `tools/list` → `{tools: [{name, description, inputSchema}]}`
//! - `tools/call` (`{name, arguments}`) → MCP `{content, isError}` with the
//!   op envelope as JSON text
//! - `ping` → `{}`
//! - `notifications/initialized` (notification) → no response
//!
//! Errors use standard JSON-RPC codes: `-32700` parse, `-32600` invalid
//! request, `-32601` unknown method, `-32602` unknown tool / bad params.
//! A failed op is NOT a JSON-RPC error: it returns `isError: true` content
//! carrying the [`crate::proto::OpError`], so agents see stable op codes.

mod serve;
mod tools;

pub use serve::*;
pub use tools::*;
