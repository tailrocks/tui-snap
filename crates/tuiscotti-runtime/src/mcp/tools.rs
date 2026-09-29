use std::io::{BufRead, Write};

use serde_json::{Value, json};

use crate::proto::{self, OpError};
use super::*;


/// MCP protocol version this server speaks.
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";


/// One MCP tool: a [`crate::proto::Op`] variant plus its hand-written schema.
#[derive(Debug, Clone)]
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}


pub(crate) fn schema(required: &[&str], properties: Value) -> Value {
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
