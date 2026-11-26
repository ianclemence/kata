//! JSON-RPC 2.0 over newline-delimited stdio, as the MCP stdio transport defines it.

use crate::tools::Server;
use crate::Config;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

pub const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Kata cuts subjects out of photos, prints them in a brand's inks, composes posters \
from a JSON scene and reviews them for the mistakes people notice. Start with kata_status. \
Read the kata://skills/kata-poster resource before designing anything, and kata://skills/kata-review \
before calling a design finished. Every file you get back has an id (art_N_kind) you can pass to \
other tools instead of a path. Use kata_view to look closely at any region: always zoom in at 100% \
on edges, logos and type before you call something done.";

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn result(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// Handle one message. Returns `None` for notifications.
pub fn handle(server: &mut Server, message: &Value) -> Option<Value> {
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(json!({}));
    // Notifications (no id) get no reply.
    let id = id?;
    let reply = match method {
        "initialize" => {
            let asked = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(PROTOCOL_VERSIONS[0]);
            let version = if PROTOCOL_VERSIONS.contains(&asked) {
                asked
            } else {
                PROTOCOL_VERSIONS[0]
            };
            result(
                &id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}, "resources": {"listChanged": false}, "prompts": {"listChanged": false}},
                    "serverInfo": {"name": "kata", "title": "Kata", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": INSTRUCTIONS
                }),
            )
        }
        "ping" => result(&id, json!({})),
        "tools/list" => result(&id, json!({"tools": crate::tools::definitions()})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match server.call(name, &args) {
                Ok(content) => result(&id, json!({"content": content, "isError": false})),
                // Tool failures are results, not protocol errors, so the agent can read them.
                Err(message) => result(
                    &id,
                    json!({"content": [{"type": "text", "text": message}], "isError": true}),
                ),
            }
        }
        "resources/list" => result(&id, json!({"resources": crate::skills::list()})),
        "resources/read" => {
            let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
            match crate::skills::read(uri) {
                Some(contents) => result(&id, json!({"contents": [contents]})),
                None => error(&id, -32002, &format!("no resource at {uri}")),
            }
        }
        "prompts/list" => result(&id, json!({"prompts": crate::skills::prompts()})),
        "prompts/get" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match crate::skills::prompt(name, &args) {
                Some(p) => result(&id, p),
                None => error(&id, -32602, &format!("no prompt called {name}")),
            }
        }
        other => error(&id, -32601, &format!("method not found: {other}")),
    };
    Some(reply)
}

/// Serve on stdin/stdout until stdin closes. Logs go to stderr only.
pub fn serve(config: Config) -> std::io::Result<()> {
    let mut server = Server::new(config)?;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                let replies: Vec<Value> = batch
                    .iter()
                    .filter_map(|m| handle(&mut server, m))
                    .collect();
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            Ok(message) => handle(&mut server, &message),
            Err(e) => Some(error(&Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(reply) = reply {
            serde_json::to_writer(&mut stdout, &reply)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }
    Ok(())
}
