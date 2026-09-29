use std::io::{BufRead, Write};

use serde_json::{Value, json};

use super::{MCP_PROTOCOL_VERSION, tools, tools_list_json};
use crate::proto::{self, OpError};

/// Serve JSON-RPC 2.0 over `reader`/`writer`, one message per line, until EOF.
/// Blank lines are ignored. Notifications get no response.
pub fn serve<R: BufRead, W: Write>(mut reader: R, mut writer: W) {
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = handle_request(&line) {
            // An unwritable client is gone: stop serving instead of spinning
            // on requests whose responses go nowhere.
            if writeln!(writer, "{resp}").is_err() {
                break;
            }
            if writer.flush().is_err() {
                break;
            }
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
                &Value::Null,
                -32700,
                "Parse error",
                Some(json!(e.to_string())),
            ));
        }
    };
    if msg.is_array() {
        return Some(error_response(
            &Value::Null,
            -32600,
            "Invalid Request: batches unsupported",
            None,
        ));
    }
    if msg.get("jsonrpc") != Some(&Value::String("2.0".to_string())) {
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        return Some(error_response(
            &id,
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
            &id,
            -32600,
            "Invalid Request: missing method",
            None,
        ));
    };
    // Notification (no id): only `notifications/*` is meaningful; never reply.
    let id = id?;
    match method {
        "initialize" => Some(success_response(&id, &initialize_result(&params))),
        "tools/list" => Some(success_response(&id, &tools_list_json())),
        "tools/call" => Some(call_tool(&id, &params)),
        "ping" => Some(success_response(&id, &json!({}))),
        m if m.starts_with("notifications/") => Some(error_response(
            &id,
            -32601,
            "Method not found: notifications take no id",
            None,
        )),
        other => Some(error_response(
            &id,
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
            "name": "tuiscotti",
            "version": env!("CARGO_PKG_VERSION"),
        },
    })
}

fn call_tool(id: &Value, params: &Value) -> String {
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
    let Some(args_obj) = args.as_object_mut() else {
        return error_response(
            id,
            -32603,
            "Internal error: arguments object unavailable",
            None,
        );
    };
    args_obj.insert("type".to_string(), Value::String(name.to_string()));
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
        &json!({
            "content": [{"type": "text", "text": text}],
            "isError": is_error,
        }),
    )
}

fn success_response(id: &Value, result: &Value) -> String {
    serde_json::to_string(&json!({"jsonrpc": "2.0", "id": id, "result": result})).unwrap_or_else(
        |_| {
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Internal error"}}"#
                .to_string()
        },
    )
}

fn error_response(id: &Value, code: i32, message: &str, data: Option<Value>) -> String {
    let mut fields = serde_json::Map::with_capacity(3);
    fields.insert("code".to_string(), json!(code));
    fields.insert("message".to_string(), Value::String(message.to_string()));
    if let Some(data) = data {
        fields.insert("data".to_string(), data);
    }
    let error = Value::Object(fields);
    serde_json::to_string(&json!({"jsonrpc": "2.0", "id": id, "error": error})).unwrap_or_else(
        |_| {
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Internal error"}}"#
                .to_string()
        },
    )
}
