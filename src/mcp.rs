use crate::AppState;
use serde_json::{json, Value};
use std::sync::Arc;

pub async fn handle_line(line: &str, state: &Arc<AppState>) -> Option<String> {
    let req: Value = serde_json::from_str(line).ok()?;
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method")?.as_str()?;

    let response = match method {
        "initialize" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "email-mcp", "version": "0.1.0" }
            }
        }),
        "notifications/initialized" => return None,
        "ping" => json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
        "tools/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "tools": crate::tools::list() }
        }),
        "tools/call" => {
            let params = req.get("params")?;
            let tool_name = params.get("name")?.as_str()?;
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let result = crate::tools::call(tool_name, args, state).await;
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result
            })
        }
        _ => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": "method not found" }
        }),
    };

    Some(serde_json::to_string(&response).ok()?)
}
