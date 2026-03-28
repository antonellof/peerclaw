//! JSON-RPC 2.0 dispatch for A2A-style methods.

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::state::A2aState;

#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
    pub id: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    fn ok(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    fn err(id: Option<serde_json::Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
            id,
        }
    }
}

/// Parse a JSON body (single request) and return JSON-RPC response object.
pub fn handle_jsonrpc_body(state: &A2aState, body: &serde_json::Value) -> serde_json::Value {
    let req: JsonRpcRequest = match serde_json::from_value(body.clone()) {
        Ok(r) => r,
        Err(e) => {
            return serde_json::to_value(JsonRpcResponse::err(
                None,
                -32700,
                format!("Parse error: {e}"),
            ))
            .unwrap_or_else(|_| json!({"jsonrpc":"2.0","error":{"code":-32700,"message":"parse error"}}));
        }
    };

    if req.jsonrpc != "2.0" {
        return serde_json::to_value(JsonRpcResponse::err(
            req.id.clone(),
            -32600,
            "Invalid Request: jsonrpc must be 2.0",
        ))
        .unwrap();
    }

    let id = req.id.clone();
    let result = match req.method.as_str() {
        "tasks/create" | "a2a.tasks.create" => {
            let hint = req
                .params
                .as_ref()
                .and_then(|p| p.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("task");
            let rec = state.create_task(hint);
            serde_json::to_value(rec).unwrap_or_else(|_| json!({}))
        }
        "tasks/get" | "tasks/getState" | "a2a.tasks.get" => {
            let tid = req
                .params
                .as_ref()
                .and_then(|p| p.get("id").or_else(|| p.get("taskId")))
                .and_then(|v| v.as_str());
            match tid {
                Some(id_str) => match state.get_task(id_str) {
                    Some(t) => serde_json::to_value(t).unwrap_or_else(|_| json!({})),
                    None => {
                        return serde_json::to_value(JsonRpcResponse::err(
                            id,
                            -32001,
                            "task not found",
                        ))
                        .unwrap();
                    }
                },
                None => {
                    return serde_json::to_value(JsonRpcResponse::err(
                        id,
                        -32602,
                        "missing params.id",
                    ))
                    .unwrap();
                }
            }
        }
        "tasks/cancel" | "a2a.tasks.cancel" => {
            let tid = req
                .params
                .as_ref()
                .and_then(|p| p.get("id").or_else(|| p.get("taskId")))
                .and_then(|v| v.as_str());
            match tid {
                Some(id_str) => {
                    if state.cancel_task(id_str) {
                        json!({"ok": true})
                    } else {
                        return serde_json::to_value(JsonRpcResponse::err(
                            id,
                            -32001,
                            "task not found",
                        ))
                        .unwrap();
                    }
                }
                None => {
                    return serde_json::to_value(JsonRpcResponse::err(
                        id,
                        -32602,
                        "missing params.id",
                    ))
                    .unwrap();
                }
            }
        }
        "message/send" | "a2a.message.send" => {
            // Minimal stub: create task, mark completed with echo payload.
            let text = req
                .params
                .as_ref()
                .and_then(|p| p.get("text").or_else(|| p.get("content")))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let rec = state.create_task("message");
            let _ = state.complete_task(
                &rec.id,
                Some(json!({
                    "role": "agent",
                    "content": format!("echo: {text}"),
                })),
            );
            match state.get_task(&rec.id) {
                Some(t) => serde_json::to_value(t).unwrap_or_else(|_| json!({})),
                None => json!({}),
            }
        }
        "agent/cards" | "peerclaw.agent.cards" => {
            let cards: Vec<serde_json::Value> = state
                .list_peer_cards()
                .into_iter()
                .map(|(pid, c)| json!({"peer_id": pid, "card": c}))
                .collect();
            json!({ "cards": cards })
        }
        _ => {
            return serde_json::to_value(JsonRpcResponse::err(
                id,
                -32601,
                format!("Method not found: {}", req.method),
            ))
            .unwrap();
        }
    };

    serde_json::to_value(JsonRpcResponse::ok(id, result)).unwrap()
}

/// libp2p wire: same JSON-RPC, wrapped for serde request-response codec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aRpcWireRequest {
    pub envelope: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aRpcWireResponse {
    pub envelope: serde_json::Value,
}

impl A2aRpcWireRequest {
    pub fn dispatch(self, state: &A2aState) -> A2aRpcWireResponse {
        A2aRpcWireResponse {
            envelope: handle_jsonrpc_body(state, &self.envelope),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasks_create_returns_task() {
        let st = A2aState::default();
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tasks/create",
            "params": {"message": "hello"}
        });
        let out = handle_jsonrpc_body(&st, &body);
        assert_eq!(out["jsonrpc"], "2.0");
        assert!(out.get("result").is_some());
        assert_eq!(out["id"], 1);
        let res = &out["result"];
        assert_eq!(res["status"], "working");
    }

    #[test]
    fn unknown_method_error() {
        let st = A2aState::default();
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "nope",
        });
        let out = handle_jsonrpc_body(&st, &body);
        assert!(out.get("error").is_some());
    }
}

