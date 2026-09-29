use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use super::*;


// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Machine-readable op failure. `code` is stable; `message` is human detail.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}


impl OpError {
    #[must_use]
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            session: None,
        }
    }

    #[must_use]
    pub fn with_session(mut self, session: &str) -> Self {
        self.session = Some(session.to_string());
        self
    }
}


impl std::fmt::Display for OpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.session {
            Some(s) => write!(f, "[{}] {s}: {}", self.code, self.message),
            None => write!(f, "[{}] {}", self.code, self.message),
        }
    }
}


impl std::error::Error for OpError {}


// ---------------------------------------------------------------------------
// Machine envelope (`--machine` JSON lines)
// ---------------------------------------------------------------------------

/// One output line of machine mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<OpResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<OpError>,
}


/// Parse one input line and execute it. Returns the output line plus whether
/// the op succeeded. Never panics on adversarial input.
pub fn run_machine_line(line: &str) -> (String, bool) {
    let env = match serde_json::from_str::<Op>(line) {
        Ok(op) => match execute(&op) {
            Ok(result) => Envelope {
                ok: true,
                result: Some(result),
                error: None,
            },
            Err(error) => Envelope {
                ok: false,
                result: None,
                error: Some(error),
            },
        },
        Err(e) => Envelope {
            ok: false,
            result: None,
            error: Some(OpError::new("invalid-input", format!("not an Op: {e}"))),
        },
    };
    let ok = env.ok;
    let line = serde_json::to_string(&env).unwrap_or_else(|_| {
        r#"{"ok":false,"error":{"code":"internal","message":"envelope serialize failed"}}"#
            .to_string()
    });
    (line, ok)
}


/// JSON Schema (draft 2020-12 subset) for [`Op`], [`OpResult`] and the machine
/// envelope. Hand-written: the protocol makes no promise a codegen schema
/// would keep (e.g. `bytes_b64` must decode).
pub const PROTOCOL_SCHEMA_JSON: &str = r##"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "tui-snap op protocol v1",
  "type": "object",
  "definitions": {
    "op": {
      "type": "object",
      "required": ["type"],
      "properties": {
        "type": {
          "type": "string",
          "enum": ["spawn", "stdin", "observe", "snapshot", "screenshot", "wait", "exit",
                   "assert", "render", "diff", "session-start", "session-stop",
                   "session-list", "version", "capabilities"]
        }
      },
      "allOf": [
        {"if": {"properties": {"type": {"const": "spawn"}}},
         "then": {"required": ["argv"],
                  "properties": {"argv": {"type": "array", "items": {"type": "string"}, "minItems": 1},
                                 "id": {"type": "string"}, "cols": {"type": "integer"},
                                 "rows": {"type": "integer"}, "cwd": {"type": "string"},
                                 "env": {"type": "object" }}}},
        {"if": {"properties": {"type": {"const": "stdin"}}},
         "then": {"required": ["session"],
                  "properties": {"session": {"type": "string"}, "text": {"type": "string"},
                                 "chord": {"type": "string"}, "bytes_b64": {"type": "string"}}}},
        {"if": {"properties": {"type": {"const": "observe"}}}, "then": {"required": ["session"]}},
        {"if": {"properties": {"type": {"const": "snapshot"}}}, "then": {"required": ["session"]}},
        {"if": {"properties": {"type": {"const": "screenshot"}}}, "then": {"required": ["session"]}},
        {"if": {"properties": {"type": {"const": "wait"}}},
         "then": {"required": ["session", "kind"],
                  "properties": {"session": {"type": "string"}, "timeout_ms": {"type": "integer"},
                                 "kind": {"enum": ["text", "stable", "exit"]},
                                 "needle": {"type": "string"}, "quiet_ms": {"type": "integer"}}}},
        {"if": {"properties": {"type": {"const": "exit"}}},
         "then": {"required": ["session"],
                  "properties": {"session": {"type": "string"}, "timeout_ms": {"type": "integer"}}}},
        {"if": {"properties": {"type": {"const": "assert"}}},
         "then": {"required": ["check"],
                  "properties": {"check": {"enum": ["text-contains", "text-equals"]},
                                 "text": {"type": "string"}, "needle": {"type": "string"},
                                 "actual": {"type": "string"}, "expected": {"type": "string"}}}},
        {"if": {"properties": {"type": {"const": "render"}}},
         "then": {"required": ["frame_json", "format"],
                  "properties": {"frame_json": {"type": "string"},
                                 "format": {"enum": ["png", "ansi", "txt", "svg", "html"]}}}},
        {"if": {"properties": {"type": {"const": "diff"}}},
         "then": {"required": ["expected_png_b64", "actual_png_b64"]}},
        {"if": {"properties": {"type": {"const": "session-start"}}},
         "then": {"required": ["name", "argv"],
                  "properties": {"name": {"type": "string"},
                                 "argv": {"type": "array", "items": {"type": "string"}},
                                 "force": {"type": "boolean"}}}},
        {"if": {"properties": {"type": {"const": "session-stop"}}}, "then": {"required": ["name"]}}
      ]
    },
    "result": {
      "type": "object",
      "required": ["type"],
      "properties": {
        "type": {
          "type": "string",
          "enum": ["spawned", "input-accepted", "observation", "snapshot", "screenshot",
                   "waited", "exited", "asserted", "rendered", "diffed", "session",
                   "session-list", "version", "capabilities"]
        }
      }
    },
    "error": {
      "type": "object",
      "required": ["code", "message"],
      "properties": {
        "code": {"type": "string",
                 "enum": ["invalid-input", "unsupported", "not-found", "spawn-failed",
                          "timeout", "cancelled", "op-failed", "io", "render",
                          "session-exists", "owner-mismatch", "version-mismatch",
                          "bound-exceeded", "internal"]},
        "message": {"type": "string"},
        "session": {"type": "string"}
      }
    }
  },
  "properties": {
    "op": {"$ref": "#/definitions/op"},
    "envelope": {
      "type": "object",
      "required": ["ok"],
      "properties": {
        "ok": {"type": "boolean"},
        "result": {"$ref": "#/definitions/result"},
        "error": {"$ref": "#/definitions/error"}
      }
    }
  }
}"##;
