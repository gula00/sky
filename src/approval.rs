use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const APPROVED_APP_META_KEY: &str = "x-oai-cua-approved-app";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub app: String,
    pub display_name: String,
    pub risk_level: String,
}

impl ApprovalRequest {
    pub fn from_app(app: impl Into<String>) -> Self {
        let app = app.into();
        Self {
            display_name: display_name_from_app_id(&app),
            app,
            risk_level: "low".to_string(),
        }
    }

    pub fn native_pipe_params(&self) -> Value {
        json!({
            "app": self.app,
            "displayName": self.display_name,
            "id": format!("computer-use-app-{}", sanitize_id_part(&self.app)),
            "message": format!("Allow Computer Use to use \"{}\"?", self.display_name),
            "riskLevel": self.risk_level,
            "risk_level": self.risk_level,
            "title": "Computer Use approval",
            "meta": {
                "codex_approval_kind": "mcp_tool_call",
                "connector_id": "computer-use",
                "connector_name": "Computer Use",
                "persist": ["session", "always"],
                "riskLevel": self.risk_level,
                "tool_params": {
                    "app": self.app
                },
                "tool_params_display": [
                    {
                        "name": "app",
                        "display_name": "App",
                        "value": self.display_name
                    }
                ]
            }
        })
    }
}

pub fn approved_app_from_meta(meta: &Value) -> Option<&str> {
    meta.get(APPROVED_APP_META_KEY)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub fn meta_approves_app(meta: &Value, app: &str) -> bool {
    match approved_app_from_meta(meta) {
        Some("*") => true,
        Some(approved) => approved.eq_ignore_ascii_case(app),
        None => false,
    }
}

pub fn approval_response_allows(value: &Value) -> bool {
    if value.as_bool() == Some(true) {
        return true;
    }

    let Some(object) = value.as_object() else {
        return false;
    };

    if object.get("approved").and_then(Value::as_bool) == Some(true) {
        return true;
    }

    let action = object
        .get("action")
        .or_else(|| object.get("status"))
        .or_else(|| object.get("result"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_ascii_lowercase());

    matches!(
        action.as_deref(),
        Some("accept" | "accepted" | "allow" | "allowed" | "approve" | "approved")
    )
}

fn display_name_from_app_id(id: &str) -> String {
    let value = id.strip_prefix("process:").unwrap_or(id);
    let normalized = value.replace('\\', "/");
    let file_name = normalized.rsplit('/').next().unwrap_or(value);
    let stem = file_name.rsplit_once('.').map_or(file_name, |(stem, _)| stem);

    if stem.is_empty() {
        id.to_string()
    } else {
        stem.to_string()
    }
}

fn sanitize_id_part(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '_' | '-' => ch,
            _ => '_',
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct WindowParam {
    app: String,
}

pub fn app_for_method(method: &str, params: &Value) -> Option<String> {
    match method {
        "activate_window"
        | "click"
        | "click_element"
        | "drag"
        | "get_window_state"
        | "perform_secondary_action"
        | "press_key"
        | "scroll"
        | "set_value"
        | "type_text" => params
            .get("window")
            .cloned()
            .and_then(|value| serde_json::from_value::<WindowParam>(value).ok())
            .map(|window| window.app),
        "launch_app" => params
            .get("app")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

pub fn approval_request_for_method(method: &str, params: &Value) -> Option<ApprovalRequest> {
    app_for_method(method, params).map(ApprovalRequest::from_app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_window_app_for_action() {
        let approval = approval_request_for_method(
            "click",
            &json!({ "window": { "app": "process:C:\\Windows\\notepad.exe", "id": 1 }}),
        )
        .unwrap();

        assert_eq!(approval.app, r"process:C:\Windows\notepad.exe");
        assert_eq!(approval.display_name, "notepad");
    }

    #[test]
    fn accepts_common_approval_shapes() {
        assert!(approval_response_allows(&json!(true)));
        assert!(approval_response_allows(&json!({ "approved": true })));
        assert!(approval_response_allows(&json!({ "action": "accept" })));
        assert!(!approval_response_allows(&json!({ "action": "deny" })));
    }
}
