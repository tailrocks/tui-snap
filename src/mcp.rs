//! MCP-ish JSON-RPC 2.0 stdio server over the typed op protocol (A08).
//!
//! One MCP tool per [`crate::proto::Op`] variant; `tools/call` executes via
//! [`crate::proto::execute`] and returns the [`crate::proto::OpResult`] /
//! [`crate::proto::OpError`] verbatim inside MCP content.
//!
//! Transport: newline-delimited JSON-RPC 2.0 on stdio, hand-rolled on
//! `serde_json` only (no new dependencies).
//!
//! Composition note: there is intentionally no `tuisnap mcp` subcommand here
//! (`main.rs` is owned by another agent). Agents either link this module
//! ([`run_stdio`]) or drive the equivalent vocabulary through the already
//! shipped `tuisnap --machine` JSON-lines protocol, which speaks the same
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

use std::io::{BufRead, Write};

use serde_json::{json, Value};

use crate::proto::{self, OpError};

/// MCP protocol version this server speaks.
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// One MCP tool: a [`crate::proto::Op`] variant plus its hand-written schema.
#[derive(Debug, Clone)]
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

fn schema(required: &[&str], properties: Value) -> Value {
    json!({
        "type": "object",
        "required": required,
        "properties": properties,
        "additionalProperties": false,
    })
}

/// Every tool, in [`crate::proto::Op`] declaration order. Schemas are written
/// from the `Op` struct fields (see `PROTOCOL_SCHEMA_JSON` for the same
/// contract in schema form); `tools/list` serializes this table.
#[must_use]
pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "spawn",
            description: "Spawn a child in a PTY session (needs `pty` feature).",
            input_schema: schema(
                &["argv"],
                json!({
                    "argv": {"type": "array", "items": {"type": "string"}, "minItems": 1},
                    "id": {"type": "string"},
                    "cols": {"type": "integer"}, "rows": {"type": "integer"},
                    "cwd": {"type": "string"},
                    "env": {"type": "object", "additionalProperties": {"type": "string"}},
                }),
            ),
        },
        Tool {
            name: "stdin",
            description: "Send exactly one of text|chord|bytes_b64 to a PTY session.",
            input_schema: schema(
                &["session"],
                json!({
                    "session": {"type": "string"},
                    "text": {"type": "string"},
                    "chord": {"type": "string"},
                    "bytes_b64": {"type": "string"},
                }),
            ),
        },
        Tool {
            name: "observe",
            description: "Current observation (revision + screen) of a PTY session.",
            input_schema: schema(&["session"], json!({"session": {"type": "string"}})),
        },
        Tool {
            name: "snapshot",
            description: "Current screen text projection of a PTY session.",
            input_schema: schema(&["session"], json!({"session": {"type": "string"}})),
        },
        Tool {
            name: "screenshot",
            description: "Screen + canonical string + PNG of a PTY session.",
            input_schema: schema(&["session"], json!({"session": {"type": "string"}})),
        },
        Tool {
            name: "wait",
            description: "Wait for text|stable|exit on a PTY session (fails on timeout).",
            input_schema: schema(
                &["session", "kind"],
                json!({
                    "session": {"type": "string"},
                    "kind": {"type": "string", "enum": ["text", "stable", "exit"]},
                    "needle": {"type": "string"},
                    "quiet_ms": {"type": "integer"},
                    "timeout_ms": {"type": "integer"},
                }),
            ),
        },
        Tool {
            name: "exit",
            description: "Wait for natural exit, reap, drop the PTY session.",
            input_schema: schema(
                &["session"],
                json!({
                    "session": {"type": "string"},
                    "timeout_ms": {"type": "integer"},
                }),
            ),
        },
        Tool {
            name: "assert",
            description: "Run a shared-engine check (text-contains|text-equals).",
            input_schema: schema(
                &["check"],
                json!({
                    "check": {"type": "string", "enum": ["text-contains", "text-equals"]},
                    "text": {"type": "string"}, "needle": {"type": "string"},
                    "actual": {"type": "string"}, "expected": {"type": "string"},
                }),
            ),
        },
        Tool {
            name: "render",
            description: "Render a frame JSON to png|ansi|txt|svg|html.",
            input_schema: schema(
                &["frame_json", "format"],
                json!({
                    "frame_json": {"type": "string"},
                    "format": {"type": "string", "enum": ["png", "ansi", "txt", "svg", "html"]},
                }),
            ),
        },
        Tool {
            name: "diff",
            description: "Compare two base64 PNGs; report equality + score.",
            input_schema: schema(
                &["expected_png_b64", "actual_png_b64"],
                json!({
                    "expected_png_b64": {"type": "string"},
                    "actual_png_b64": {"type": "string"},
                }),
            ),
        },
        Tool {
            name: "session-start",
            description: "Start a named (piped, detached) session; force replaces a live one.",
            input_schema: schema(
                &["name", "argv"],
                json!({
                    "name": {"type": "string"},
                    "argv": {"type": "array", "items": {"type": "string"}},
                    "force": {"type": "boolean"},
                }),
            ),
        },
        Tool {
            name: "session-stop",
            description: "Stop a named session; returns last-known info.",
            input_schema: schema(&["name"], json!({"name": {"type": "string"}})),
        },
        Tool {
            name: "session-list",
            description: "List named sessions with liveness.",
            input_schema: schema(&[], json!({})),
        },
        Tool {
            name: "version",
            description: "Protocol + tuisnap versions.",
            input_schema: schema(&[], json!({})),
        },
        Tool {
            name: "capabilities",
            description: "What this build can do (pty/render/record/platform).",
            input_schema: schema(&[], json!({})),
        },
    ]
}

/// The `tools/list` result value (also the schema-snapshot source).
#[must_use]
pub fn tools_list_json() -> Value {
    let tools: Vec<Value> = tools()
        .into_iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema,
            })
        })
        .collect();
    json!({ "tools": tools })
}

/// Serve JSON-RPC 2.0 over `reader`/`writer`, one message per line, until EOF.
/// Blank lines are ignored. Notifications get no response.
pub fn serve<R: BufRead, W: Write>(mut reader: R, mut writer: W) {
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = handle_request(&line) {
            let _ = writeln!(writer, "{resp}");
            let _ = writer.flush();
        }
    }
}

/// Serve on the real stdio. See the module docs for composition notes.
pub fn run_stdio() {
    let stdin = std::io::stdin();
    serve(stdin.lock(), std::io::stdout());
}

/// Handle one raw JSON-RPC message. Returns `None` for notifications
/// (no `id`) and for anything that must stay silent.
pub fn handle_request(raw: &str) -> Option<String> {
    let msg: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(e) => {
            return Some(error_response(
                Value::Null,
                -32700,
                "Parse error",
                Some(json!(e.to_string())),
            ))
        }
    };
    if msg.is_array() {
        return Some(error_response(
            Value::Null,
            -32600,
            "Invalid Request: batches unsupported",
            None,
        ));
    }
    if msg.get("jsonrpc") != Some(&Value::String("2.0".to_string())) {
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        return Some(error_response(
            id,
            -32600,
            "Invalid Request: want {\"jsonrpc\":\"2.0\",...}",
            None,
        ));
    }
    let method = msg.get("method").and_then(Value::as_str);
    let id = msg.get("id").cloned();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let Some(method) = method else {
        // No method and no id: pure noise, stay silent.
        let id = id?;
        return Some(error_response(
            id,
            -32600,
            "Invalid Request: missing method",
            None,
        ));
    };
    // Notification (no id): only `notifications/*` is meaningful; never reply.
    let id = id?;
    match method {
        "initialize" => Some(success_response(id, initialize_result(&params))),
        "tools/list" => Some(success_response(id, tools_list_json())),
        "tools/call" => Some(call_tool(id, &params)),
        "ping" => Some(success_response(id, json!({}))),
        m if m.starts_with("notifications/") => Some(error_response(
            id,
            -32601,
            "Method not found: notifications take no id",
            None,
        )),
        other => Some(error_response(
            id,
            -32601,
            "Method not found",
            Some(json!(other)),
        )),
    }
}

fn initialize_result(params: &Value) -> Value {
    let client = params
        .get("clientInfo")
        .and_then(|c| c.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let _ = client;
    json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "tuisnap",
            "version": env!("CARGO_PKG_VERSION"),
        },
    })
}

fn call_tool(id: Value, params: &Value) -> String {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let tool = tools().into_iter().find(|t| t.name == name);
    if name.is_empty() || tool.is_none() {
        return error_response(
            id,
            -32602,
            "Invalid params: unknown tool",
            Some(json!(name)),
        );
    }
    let mut args = params.get("arguments").cloned().unwrap_or(json!({}));
    if !args.is_object() {
        return error_response(
            id,
            -32602,
            "Invalid params: arguments must be an object",
            None,
        );
    }
    args.as_object_mut()
        .expect("checked object")
        .insert("type".to_string(), Value::String(name.to_string()));
    let op: proto::Op = match serde_json::from_value(args) {
        Ok(op) => op,
        Err(e) => {
            return error_response(
                id,
                -32602,
                "Invalid params: arguments do not match the tool schema",
                Some(json!(e.to_string())),
            );
        }
    };
    let (envelope, is_error) = match proto::execute(&op) {
        Ok(result) => (json!({"ok": true, "result": result}), false),
        Err(error) => (json!({"ok": false, "error": error}), true),
    };
    let text = serde_json::to_string(&envelope).unwrap_or_else(|_| {
        let fallback = OpError::new("internal", "envelope serialize failed");
        format!(
            "{{\"ok\":false,\"error\":{}}}",
            serde_json::to_string(&fallback).unwrap_or_default()
        )
    });
    success_response(
        id,
        json!({
            "content": [{"type": "text", "text": text}],
            "isError": is_error,
        }),
    )
}

fn success_response(id: Value, result: Value) -> String {
    serde_json::to_string(&json!({"jsonrpc": "2.0", "id": id, "result": result})).unwrap_or_else(
        |_| {
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Internal error"}}"#
                .to_string()
        },
    )
}

fn error_response(id: Value, code: i32, message: &str, data: Option<Value>) -> String {
    let mut error = json!({"code": code, "message": message});
    if let Some(data) = data {
        error
            .as_object_mut()
            .expect("object")
            .insert("data".to_string(), data);
    }
    serde_json::to_string(&json!({"jsonrpc": "2.0", "id": id, "error": error})).unwrap_or_else(
        |_| {
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Internal error"}}"#
                .to_string()
        },
    )
}
