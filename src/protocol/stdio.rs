use crate::approval::ApprovalRequest;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct StdioRequest {
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub meta: Value,
}

#[derive(Debug, Serialize)]
pub struct StdioResponse {
    pub id: Value,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "approvalRequest")]
    pub approval_request: Option<ApprovalRequest>,
    #[serde(skip)]
    pub should_close: bool,
}

impl StdioResponse {
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            id,
            ok: true,
            result: Some(result),
            error: None,
            approval_request: None,
            should_close: false,
        }
    }

    pub fn closing(id: Value, result: Value) -> Self {
        Self {
            should_close: true,
            ..Self::success(id, result)
        }
    }

    pub fn error(id: Value, message: impl Into<String>) -> Self {
        Self {
            id,
            ok: false,
            result: None,
            error: Some(message.into()),
            approval_request: None,
            should_close: false,
        }
    }

    pub fn approval_required(id: Value, request: ApprovalRequest) -> Self {
        Self {
            id,
            ok: false,
            result: None,
            error: None,
            approval_request: Some(request),
            should_close: false,
        }
    }
}
