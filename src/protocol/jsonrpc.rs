use crate::{
    approval::{approval_request_for_method, approval_response_allows, ApprovalRequest},
    budget::check_request_budget,
    router::{self, RouterOutcome},
    turn,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const INTERNAL_ERROR: i32 = -32000;

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub id: Value,
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Deserialize)]
struct ComputerUseRequestParams {
    #[serde(default, rename = "codexTurnMetadata")]
    codex_turn_metadata: Value,
    #[serde(default)]
    meta: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub id: Value,
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    #[serde(skip)]
    pub should_close: bool,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

impl JsonRpcResponse {
    pub fn result(id: Value, result: Value) -> Self {
        Self {
            id,
            jsonrpc: "2.0",
            result: Some(result),
            error: None,
            should_close: false,
        }
    }

    pub fn closing(id: Value, result: Value) -> Self {
        Self {
            should_close: true,
            ..Self::result(id, result)
        }
    }

    pub fn error(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            id,
            jsonrpc: "2.0",
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
            }),
            should_close: false,
        }
    }
}

#[allow(dead_code)]
pub fn handle_jsonrpc_request(request: JsonRpcRequest) -> JsonRpcResponse {
    handle_jsonrpc_request_with_approval(request, |_| {
        Err("Computer Use approval callback is unavailable.".to_string())
    })
}

pub fn handle_jsonrpc_request_with_approval(
    request: JsonRpcRequest,
    request_approval: impl FnMut(ApprovalRequest) -> Result<Value, String>,
) -> JsonRpcResponse {
    handle_jsonrpc_request_with_approval_and_turn_state(
        request,
        &mut turn::ActiveTurnState::default(),
        request_approval,
    )
}

pub fn handle_jsonrpc_request_with_approval_and_turn_state(
    request: JsonRpcRequest,
    turn_state: &mut turn::ActiveTurnState,
    mut request_approval: impl FnMut(ApprovalRequest) -> Result<Value, String>,
) -> JsonRpcResponse {
    match request.method.as_str() {
        "ping" => JsonRpcResponse::result(request.id, Value::String("pong".to_string())),
        "close" => {
            turn_state.observe_request("close", &Value::Null);
            JsonRpcResponse::closing(request.id, Value::Null)
        }
        "request" => handle_computer_use_request(
            request.id,
            request.params,
            turn_state,
            &mut request_approval,
        ),
        _ => JsonRpcResponse::error(
            request.id,
            INTERNAL_ERROR,
            format!("Unsupported Computer Use native pipe method: {}", request.method),
        ),
    }
}

fn handle_computer_use_request(
    id: Value,
    params: Value,
    turn_state: &mut turn::ActiveTurnState,
    request_approval: &mut impl FnMut(ApprovalRequest) -> Result<Value, String>,
) -> JsonRpcResponse {
    let parsed = serde_json::from_value::<ComputerUseRequestParams>(params);
    let params = match parsed {
        Ok(params) => params,
        Err(error) => {
            return JsonRpcResponse::error(
                id,
                INTERNAL_ERROR,
                format!("Invalid Computer Use request params: {error}"),
            );
        }
    };

    turn_state.observe_request(&params.method, &params.codex_turn_metadata);

    if !turn::method_can_bypass_interrupt(&params.method)
        && turn::is_interrupted(&params.codex_turn_metadata)
    {
        return JsonRpcResponse::error(id, INTERNAL_ERROR, turn::STOPPED_BY_USER_MESSAGE);
    }

    let budget_meta = budget_metadata(&params.codex_turn_metadata, &params.meta);
    let budget = match check_request_budget(&budget_meta, &params.method) {
        Ok(budget) => budget,
        Err(error) => return JsonRpcResponse::error(id, INTERNAL_ERROR, error),
    };

    if let Some(approval_request) = approval_request_for_method(&params.method, &params.params) {
        match request_approval(approval_request.clone()) {
            Ok(result) if approval_response_allows(&result) => {}
            Ok(_) => {
                return JsonRpcResponse::error(
                    id,
                    INTERNAL_ERROR,
                    format!(
                        "Computer Use was not approved to use {}",
                        approval_request.display_name
                    ),
                );
            }
            Err(error) => return JsonRpcResponse::error(id, INTERNAL_ERROR, error),
        }
    }

    if let Some(budget) = &budget {
        if let Err(error) = budget.check(&params.method) {
            return JsonRpcResponse::error(id, INTERNAL_ERROR, error);
        }
    }

    match router::handle_method(&params.method, params.params) {
        RouterOutcome::Result(result) => JsonRpcResponse::result(id, result),
        RouterOutcome::Close(result) => JsonRpcResponse::closing(id, result),
        RouterOutcome::Error(message) => JsonRpcResponse::error(id, INTERNAL_ERROR, message),
    }
}

fn budget_metadata(codex_turn_metadata: &Value, meta: &Value) -> Value {
    if meta.is_null() {
        codex_turn_metadata.clone()
    } else if codex_turn_metadata.is_null() {
        meta.clone()
    } else {
        let mut merged = meta.clone();
        if let Some(object) = merged.as_object_mut() {
            object
                .entry("codexTurnMetadata")
                .or_insert_with(|| codex_turn_metadata.clone());
        }
        merged
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn ping_returns_pong() {
        let response = handle_jsonrpc_request(JsonRpcRequest {
            id: json!(1),
            jsonrpc: "2.0".to_string(),
            method: "ping".to_string(),
            params: Value::Null,
        });

        assert_eq!(response.result, Some(json!("pong")));
        assert!(response.error.is_none());
    }

    #[test]
    fn request_routes_inner_method() {
        let response = handle_jsonrpc_request_with_approval(
            JsonRpcRequest {
                id: json!(1),
                jsonrpc: "2.0".to_string(),
                method: "request".to_string(),
                params: json!({
                    "codexTurnMetadata": {},
                    "method": "list_windows",
                    "params": {}
                }),
            },
            |_| Ok(json!({ "approved": true })),
        );

        assert!(response.result.unwrap().is_array());
        assert!(response.error.is_none());
    }

    #[test]
    fn request_requires_approval_for_window_action() {
        let response = handle_jsonrpc_request_with_approval(
            JsonRpcRequest {
                id: json!(1),
                jsonrpc: "2.0".to_string(),
                method: "request".to_string(),
                params: json!({
                    "codexTurnMetadata": {},
                    "method": "click",
                    "params": {
                        "window": { "app": "process:C:\\Windows\\notepad.exe", "id": 1 },
                        "x": 1,
                        "y": 1
                    }
                }),
            },
            |_| Ok(json!({ "action": "deny" })),
        );

        assert!(response.error.unwrap().message.contains("not approved"));
    }

    #[test]
    fn request_rejects_interrupted_turn_before_approval() {
        let codex_home = test_codex_home("jsonrpc_request_rejects_interrupted_turn");
        let scope = turn::TurnScope {
            codex_home: codex_home.clone(),
            session_id: "session/1".to_string(),
            turn_id: "turn:2".to_string(),
        };
        turn::write_interrupt_file(&scope).unwrap();

        let response = handle_jsonrpc_request_with_approval(
            JsonRpcRequest {
                id: json!(1),
                jsonrpc: "2.0".to_string(),
                method: "request".to_string(),
                params: json!({
                    "codexTurnMetadata": {
                        "codexHome": codex_home,
                        "session_id": "session/1",
                        "turn_id": "turn:2"
                    },
                    "method": "click",
                    "params": {
                        "window": { "app": "process:C:\\Windows\\notepad.exe", "id": 1 },
                        "x": 1,
                        "y": 1
                    }
                }),
            },
            |_| panic!("approval should not be requested for an interrupted turn"),
        );

        assert_eq!(
            response.error.unwrap().message,
            turn::STOPPED_BY_USER_MESSAGE
        );
    }

    #[test]
    fn request_rejects_expired_budget_before_routing() {
        let response = handle_jsonrpc_request_with_approval(
            JsonRpcRequest {
                id: json!(1),
                jsonrpc: "2.0".to_string(),
                method: "request".to_string(),
                params: json!({
                    "codexTurnMetadata": {
                        crate::budget::REQUEST_BUDGET_META_KEY: 0
                    },
                    "method": "list_windows",
                    "params": {}
                }),
            },
            |_| panic!("approval should not be requested when budget is expired"),
        );

        assert!(response.error.unwrap().message.contains("request budget"));
    }

    #[test]
    fn close_marks_response_for_shutdown() {
        let response = handle_jsonrpc_request(JsonRpcRequest {
            id: json!("close-1"),
            jsonrpc: "2.0".to_string(),
            method: "close".to_string(),
            params: Value::Null,
        });

        assert!(response.should_close);
        assert_eq!(response.result, Some(Value::Null));
    }

    #[test]
    fn unsupported_method_returns_internal_error() {
        let response = handle_jsonrpc_request(JsonRpcRequest {
            id: json!(1),
            jsonrpc: "2.0".to_string(),
            method: "unknown".to_string(),
            params: Value::Null,
        });

        assert_eq!(response.error.unwrap().code, INTERNAL_ERROR);
    }

    fn test_codex_home(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir()
            .join("sky-tests")
            .join(format!("{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }
}

