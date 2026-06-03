//! rust-faf-worker — Rust → WASM FAF MCP tool executor on Cloudflare Workers.
//!
//! The edge-native backend behind `mcpaas.live/rust/mcp/v1`: the TS RC edge owns
//! the MCP protocol (initialize / 2026-07-28 / tracing) and forwards `tools/list`
//! and `tools/call` here as plain JSON-RPC POSTs. This Worker runs the FAF tools
//! in Rust → WASM, at every Cloudflare edge — no Cloud Run hop.
//!
//! Tools are the FS-free "vROM-read" set (parse/validate/score); write tools that
//! need a filesystem stay in the native `rust-faf-mcp` stdio binary.

use serde_json::{json, Value};
use worker::*;

#[event(fetch)]
async fn fetch(mut req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    // Browser GET → a friendly note. MCP clients POST JSON-RPC.
    if req.method() != Method::Post {
        return Response::ok(
            "rust-faf-worker — Rust→WASM FAF tools at the edge. POST JSON-RPC (tools/list, tools/call).",
        );
    }

    let body: Value = match req.json().await {
        Ok(v) => v,
        Err(_) => return rpc_error(Value::Null, -32700, "Parse error: invalid JSON"),
    };

    let id = body.get("id").cloned().unwrap_or(Value::Null);
    let method = body.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = body.get("params").cloned().unwrap_or_else(|| json!({}));

    let result = match method {
        "tools/list" => json!({ "tools": tool_list() }),
        "tools/call" => handle_call(&params),
        "ping" => json!({}),
        other => return rpc_error(id, -32601, &format!("Method not found: {}", other)),
    };

    Response::from_json(&json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn rpc_error(id: Value, code: i32, message: &str) -> Result<Response> {
    Response::from_json(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    }))
}

/// MCP tools/call result envelope.
fn tool_text(text: String, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

fn handle_call(params: &Value) -> Value {
    let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let content = args.get("content").and_then(|c| c.as_str()).unwrap_or("");

    if content.is_empty() {
        return tool_text("Provide `.faf` content via the `content` argument.".into(), true);
    }

    match name {
        "faf_validate" => match faf_rust_sdk::parse(content) {
            Ok(faf) => {
                let v = faf_rust_sdk::validate(&faf);
                tool_text(
                    json!({
                        "valid": v.valid,
                        "score": v.score,
                        "errors": v.errors,
                        "warnings": v.warnings
                    })
                    .to_string(),
                    false,
                )
            }
            Err(e) => tool_text(format!("parse error: {}", e), true),
        },
        "faf_score" => match faf_rust_sdk::parse(content) {
            Ok(faf) => {
                let v = faf_rust_sdk::validate(&faf);
                tool_text(format!("FAF score: {}%", v.score), false)
            }
            Err(e) => tool_text(format!("parse error: {}", e), true),
        },
        "faf_read" => match faf_rust_sdk::parse(content) {
            Ok(faf) => match faf_rust_sdk::stringify(&faf) {
                Ok(s) => tool_text(s, false),
                Err(e) => tool_text(format!("stringify error: {}", e), true),
            },
            Err(e) => tool_text(format!("parse error: {}", e), true),
        },
        other => tool_text(format!("unknown tool: {}", other), true),
    }
}

fn tool_list() -> Value {
    let content_schema = json!({
        "type": "object",
        "properties": { "content": { "type": "string", "description": "Raw .faf file content (YAML)" } },
        "required": ["content"]
    });
    json!([
        {
            "name": "faf_validate",
            "description": "Validate a .faf and return completeness score + errors/warnings (Rust→WASM, at the edge).",
            "inputSchema": content_schema
        },
        {
            "name": "faf_score",
            "description": "Score a .faf — 0-100 completeness (Rust→WASM, at the edge).",
            "inputSchema": content_schema
        },
        {
            "name": "faf_read",
            "description": "Parse a .faf and return its normalized structure (Rust→WASM, at the edge).",
            "inputSchema": content_schema
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // A valid minimal .faf (mirrors faf-rust-sdk/examples/minimal.faf).
    const MINIMAL_FAF: &str =
        "faf_version: 2.5.0\nproject:\n  name: minimal-example\n  goal: Simplest valid FAF file\n";

    fn call(name: &str, content: &str) -> Value {
        handle_call(&json!({ "name": name, "arguments": { "content": content } }))
    }

    #[test]
    fn tool_list_advertises_the_three_tools() {
        let tools = tool_list();
        let arr = tools.as_array().expect("tools is an array");
        assert_eq!(arr.len(), 3);
        let names: Vec<&str> = arr.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"faf_validate"));
        assert!(names.contains(&"faf_score"));
        assert!(names.contains(&"faf_read"));
    }

    #[test]
    fn faf_score_returns_a_percentage() {
        let r = call("faf_score", MINIMAL_FAF);
        assert_eq!(r["isError"], false);
        let txt = r["content"][0]["text"].as_str().unwrap();
        assert!(txt.contains("FAF score:"), "got: {txt}");
        assert!(txt.contains('%'), "got: {txt}");
    }

    #[test]
    fn faf_validate_returns_score_valid_and_errors() {
        let r = call("faf_validate", MINIMAL_FAF);
        assert_eq!(r["isError"], false);
        let txt = r["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(txt).expect("valid JSON body");
        assert!(parsed["score"].is_number());
        assert!(parsed["valid"].is_boolean());
        assert!(parsed["errors"].is_array());
        assert!(parsed["warnings"].is_array());
    }

    #[test]
    fn faf_read_returns_the_parsed_project() {
        let r = call("faf_read", MINIMAL_FAF);
        assert_eq!(r["isError"], false);
        assert!(r["content"][0]["text"].as_str().unwrap().contains("minimal-example"));
    }

    #[test]
    fn invalid_faf_is_a_clean_error_not_a_panic() {
        let r = call("faf_score", "not a valid faf file");
        assert_eq!(r["isError"], true);
        assert!(r["content"][0]["text"].as_str().unwrap().contains("error"));
    }

    #[test]
    fn empty_content_is_rejected() {
        let r = call("faf_score", "");
        assert_eq!(r["isError"], true);
    }

    #[test]
    fn unknown_tool_errors_cleanly() {
        let r = call("faf_nonexistent", MINIMAL_FAF);
        assert_eq!(r["isError"], true);
        assert!(r["content"][0]["text"].as_str().unwrap().contains("unknown tool"));
    }
}
