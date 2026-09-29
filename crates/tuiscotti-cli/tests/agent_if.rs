//! Agent interface tests (A08/A09): MCP JSON-RPC round trips over piped
//! buffers, committed schema snapshot, thin-client e2e against the built
//! `tuisnap` binary.

use std::io::{BufReader, Cursor};
use std::process::Command;

use serde_json::{json, Value};
use tuiscotti::mcp;

// ---------------------------------------------------------------------------
// Harness: drive mcp::serve over in-memory pipes
// ---------------------------------------------------------------------------

fn roundtrip(requests: &[Value]) -> Vec<Value> {
    let input = requests
        .iter()
        .map(|r| serde_json::to_string(r).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let mut out: Vec<u8> = Vec::new();
    mcp::serve(BufReader::new(Cursor::new(input)), &mut out);
    let text = String::from_utf8(out).unwrap();
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn roundtrip_raw(lines: &[&str]) -> Vec<Value> {
    let input = lines.join("\n") + "\n";
    let mut out: Vec<u8> = Vec::new();
    mcp::serve(BufReader::new(Cursor::new(input)), &mut out);
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn req(id: i64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

/// Unwrap MCP content[0].text as JSON + isError flag.
fn content_json(resp: &Value) -> (Value, bool) {
    let result = &resp["result"];
    let text = result["content"][0]["text"].as_str().unwrap();
    (
        serde_json::from_str(text).unwrap(),
        result["isError"].as_bool().unwrap(),
    )
}

// ---------------------------------------------------------------------------
// A08: JSON-RPC round trips
// ---------------------------------------------------------------------------

#[test]
fn initialize_reports_protocol_and_capabilities() {
    let resps = roundtrip(&[req(
        1,
        "initialize",
        json!({"protocolVersion": "2024-11-05", "capabilities": {},
               "clientInfo": {"name": "test", "version": "0"}}),
    )]);
    assert_eq!(resps.len(), 1);
    let r = &resps[0];
    assert_eq!(r["jsonrpc"], "2.0");
    assert_eq!(r["id"], 1);
    assert_eq!(r["result"]["protocolVersion"], mcp::MCP_PROTOCOL_VERSION);
    assert_eq!(r["result"]["capabilities"], json!({"tools": {}}));
    assert_eq!(r["result"]["serverInfo"]["name"], "tuisnap");
}

#[test]
fn tools_list_has_one_tool_per_op() {
    let resps = roundtrip(&[req(2, "tools/list", json!({}))]);
    let tools = resps[0]["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "spawn",
            "stdin",
            "observe",
            "snapshot",
            "screenshot",
            "wait",
            "exit",
            "assert",
            "render",
            "diff",
            "session-start",
            "session-stop",
            "session-list",
            "version",
            "capabilities",
        ]
    );
    for t in tools {
        assert!(t["description"].as_str().unwrap().len() > 10, "{t}");
        assert_eq!(t["inputSchema"]["type"], "object");
    }
}

#[test]
fn tools_call_version_roundtrip() {
    let resps = roundtrip(&[req(
        3,
        "tools/call",
        json!({"name": "version", "arguments": {}}),
    )]);
    assert_eq!(resps[0]["id"], 3);
    let (env, is_error) = content_json(&resps[0]);
    assert!(!is_error);
    assert_eq!(env["ok"], true);
    assert_eq!(env["result"]["type"], "version");
    assert_eq!(env["result"]["protocol"], tuiscotti::proto::PROTOCOL_VERSION);
}

#[test]
fn tools_call_runs_asserts_in_shared_engine() {
    // Passing check: op ok, verdict true.
    let resps = roundtrip(&[req(
        4,
        "tools/call",
        json!({"name": "assert", "arguments":
            {"check": "text-contains", "text": "hello", "needle": "ell"}}),
    )]);
    let (env, is_error) = content_json(&resps[0]);
    assert!(!is_error);
    assert_eq!(
        env["result"],
        json!({"type": "asserted", "passed": true, "detail": "text contains \"ell\""})
    );

    // Failing check: op still ok (verdict false travels in the result).
    let resps = roundtrip(&[req(
        5,
        "tools/call",
        json!({"name": "assert", "arguments":
            {"check": "text-equals", "actual": "a", "expected": "b"}}),
    )]);
    let (env, is_error) = content_json(&resps[0]);
    assert!(!is_error);
    assert_eq!(env["result"]["passed"], false);

    // Unknown check: op error travels verbatim with isError.
    let resps = roundtrip(&[req(
        6,
        "tools/call",
        json!({"name": "assert", "arguments": {"check": "nope"}}),
    )]);
    let (env, is_error) = content_json(&resps[0]);
    assert!(is_error);
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "invalid-input");
}

#[test]
fn unknown_tool_is_invalid_params() {
    let resps = roundtrip(&[req(
        7,
        "tools/call",
        json!({"name": "frobnicate", "arguments": {}}),
    )]);
    assert_eq!(resps[0]["error"]["code"], -32602);
}

#[test]
fn bad_params_are_invalid_params() {
    // Missing required argv.
    let resps = roundtrip(&[req(
        8,
        "tools/call",
        json!({"name": "spawn", "arguments": {}}),
    )]);
    assert_eq!(resps[0]["error"]["code"], -32602);
    // Non-object arguments.
    let resps = roundtrip(&[req(
        9,
        "tools/call",
        json!({"name": "version", "arguments": [1]}),
    )]);
    assert_eq!(resps[0]["error"]["code"], -32602);
    // Missing tool name.
    let resps = roundtrip(&[req(10, "tools/call", json!({"arguments": {}}))]);
    assert_eq!(resps[0]["error"]["code"], -32602);
    // Wrong field type: timeout_ms string instead of integer.
    let resps = roundtrip(&[req(
        11,
        "tools/call",
        json!({"name": "exit", "arguments": {"session": "s", "timeout_ms": "soon"}}),
    )]);
    assert_eq!(resps[0]["error"]["code"], -32602);
}

#[test]
fn unknown_method_is_method_not_found() {
    let resps = roundtrip(&[req(12, "resources/read", json!({}))]);
    assert_eq!(resps[0]["error"]["code"], -32601);
}

#[test]
fn malformed_input_gets_jsonrpc_errors() {
    // Parse error: id null.
    let resps = roundtrip_raw(&["{not json"]);
    assert_eq!(resps[0]["error"]["code"], -32700);
    assert_eq!(resps[0]["id"], Value::Null);
    // Missing method with id: invalid request.
    let resps = roundtrip_raw(&[r#"{"jsonrpc":"2.0","id":1}"#]);
    assert_eq!(resps[0]["error"]["code"], -32600);
    // Batch array: rejected (single-message server).
    let resps = roundtrip_raw(&[r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#]);
    assert_eq!(resps[0]["error"]["code"], -32600);
}

#[test]
fn notifications_and_blank_lines_stay_silent() {
    let resps = roundtrip_raw(&[
        "",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "   ",
        r#"{"jsonrpc":"2.0","id":13,"method":"ping"}"#,
    ]);
    assert_eq!(resps.len(), 1);
    assert_eq!(resps[0]["id"], 13);
    assert_eq!(resps[0]["result"], json!({}));
}

#[test]
fn session_tools_present_and_callable() {
    // session-list runs against a temp runtime dir so the test is hermetic
    // (explicit override: `set_var` is an `unsafe fn` in edition 2024).
    let dir = tempfile::tempdir().unwrap();
    tuiscotti::proto::set_runtime_dir_override(Some(dir.path().to_path_buf()));
    let resps = roundtrip(&[req(
        14,
        "tools/call",
        json!({"name": "session-list", "arguments": {}}),
    )]);
    let (env, is_error) = content_json(&resps[0]);
    assert!(!is_error, "{env}");
    assert_eq!(env["result"]["type"], "session-list");
    assert_eq!(env["result"]["sessions"], json!([]));
}

// ---------------------------------------------------------------------------
// A08: committed schema snapshot (reviewed; regenerate from tools_list_json)
// ---------------------------------------------------------------------------

#[test]
fn schema_snapshot_matches_committed_json() {
    let actual = serde_json::to_string_pretty(&mcp::tools_list_json()).unwrap() + "\n";
    let committed = include_str!("agent_if_tools.json");
    assert_eq!(
        actual, committed,
        "tools/list schema drift: review + update agent_if_tools.json"
    );
}

// ---------------------------------------------------------------------------
// A09: thin-client e2e against the built binary
// ---------------------------------------------------------------------------

fn tuisnap_bin() -> Option<&'static str> {
    option_env!("CARGO_BIN_EXE_tuisnap")
}

fn have(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn ts_client_example_e2e() {
    let Some(bin) = tuisnap_bin() else {
        eprintln!("warning: tuisnap binary not built; skipping ts client e2e");
        return;
    };
    if !have("node") {
        eprintln!("warning: node missing; skipping ts client e2e");
        return;
    }
    let out = Command::new("node")
        .arg("clients/ts/example.js")
        .env("TUISNAP_BIN", bin)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "node example failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<Value> = stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["type"], "version");
    assert_eq!(lines[1]["type"], "capabilities");
}

#[test]
fn py_client_example_e2e() {
    let Some(bin) = tuisnap_bin() else {
        eprintln!("warning: tuisnap binary not built; skipping py client e2e");
        return;
    };
    if !have("python3") {
        eprintln!("warning: python3 missing; skipping py client e2e");
        return;
    }
    let out = Command::new("python3")
        .arg("clients/py/example.py")
        .env("TUISNAP_BIN", bin)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "python example failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<Value> = stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["type"], "version");
    assert_eq!(lines[1]["type"], "capabilities");
}

#[test]
fn ts_client_propagates_op_errors_verbatim() {
    let Some(bin) = tuisnap_bin() else {
        eprintln!("warning: tuisnap binary not built; skipping ts error probe");
        return;
    };
    if !have("node") {
        eprintln!("warning: node missing; skipping ts error probe");
        return;
    }
    let probe = r#"
const { Client } = require('./clients/ts/index.js');
(async () => {
  const c = new Client();
  try {
    await c.call({ type: 'assert', check: 'nope' });
    console.log('NO-ERROR');
  } catch (e) {
    console.log(JSON.stringify({ code: e.code, message: e.message }));
  } finally {
    await c.close();
  }
})();
"#;
    let out = Command::new("node")
        .args(["-e", probe])
        .env("TUISNAP_BIN", bin)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "ts probe failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(String::from_utf8(out.stdout).unwrap().trim()).unwrap();
    assert_eq!(v["code"], "invalid-input");
    assert!(v["message"]
        .as_str()
        .unwrap()
        .contains("unknown assert check"));
}

#[test]
fn py_client_propagates_op_errors_verbatim() {
    let Some(bin) = tuisnap_bin() else {
        eprintln!("warning: tuisnap binary not built; skipping py error probe");
        return;
    };
    if !have("python3") {
        eprintln!("warning: python3 missing; skipping py error probe");
        return;
    }
    let probe = r#"
import json, sys
sys.path.insert(0, 'clients/py')
from tuisnap_client import Client, OpError
with Client() as c:
    try:
        c.call({"type": "assert", "check": "nope"})
        print(json.dumps({"code": "NO-ERROR"}))
    except OpError as e:
        print(json.dumps({"code": e.code, "message": str(e)}))
"#;
    let out = Command::new("python3")
        .args(["-c", probe])
        .env("TUISNAP_BIN", bin)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "py probe failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(String::from_utf8(out.stdout).unwrap().trim()).unwrap();
    assert_eq!(v["code"], "invalid-input");
    assert!(v["message"]
        .as_str()
        .unwrap()
        .contains("unknown assert check"));
}
