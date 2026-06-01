use crate::windows::{discovery, input};
use serde_json::{json, Value};

#[derive(Debug, PartialEq)]
pub enum RouterOutcome {
    Result(Value),
    Close(Value),
    Error(String),
}

pub fn handle_method(method: &str, params: Value) -> RouterOutcome {
    match method {
        "close" => RouterOutcome::Close(Value::Null),
        "end_turn" => RouterOutcome::Result(Value::Null),
        "diagnostic_state" => RouterOutcome::Result(json!({
            "name": "sky",
            "version": env!("CARGO_PKG_VERSION"),
            "protocol": "stdio-newline-json"
        })),
        "list_windows" => json_result(discovery::list_windows()),
        "list_apps" => json_result(discovery::list_apps()),
        "launch_app" => unit_result(parse_and_call(params, input::launch_app)),
        "get_window" => match serde_json::from_value(params) {
            Ok(input) => json_result(discovery::get_window(input)),
            Err(error) => RouterOutcome::Error(format!("parse get_window params: {error}")),
        },
        "get_window_state" => match serde_json::from_value(params) {
            Ok(input) => json_result(discovery::get_window_state(input)),
            Err(error) => RouterOutcome::Error(format!("parse get_window_state params: {error}")),
        },
        "activate_window" => unit_result(parse_and_call(params, input::activate_window)),
        "click" => unit_result(parse_and_call(params, input::click)),
        "click_element" => unit_result(parse_and_call(params, input::click_element)),
        "press_key" => unit_result(parse_and_call(params, input::press_key)),
        "type_text" => unit_result(parse_and_call(params, input::type_text)),
        "scroll" => unit_result(parse_and_call(params, input::scroll)),
        "drag" => unit_result(parse_and_call(params, input::drag)),
        "set_value" => unit_result(parse_and_call(params, input::set_value)),
        "perform_secondary_action" => {
            unit_result(parse_and_call(params, input::perform_secondary_action))
        }
        "scroll_element" => {
            RouterOutcome::Error(format!("{method} is not implemented yet"))
        }
        _ => RouterOutcome::Error(format!("unsupported method: {method}")),
    }
}

fn json_result<T: serde::Serialize>(result: anyhow::Result<T>) -> RouterOutcome {
    match result {
        Ok(value) => match serde_json::to_value(value) {
            Ok(value) => RouterOutcome::Result(value),
            Err(error) => RouterOutcome::Error(format!("failed to serialize result: {error}")),
        },
        Err(error) => RouterOutcome::Error(error.to_string()),
    }
}

fn parse_and_call<T>(
    params: Value,
    action: impl FnOnce(T) -> anyhow::Result<()>,
) -> anyhow::Result<()> 
where
    T: serde::de::DeserializeOwned,
{
    let input = serde_json::from_value(params)?;
    action(input)
}

fn unit_result(result: anyhow::Result<()>) -> RouterOutcome {
    match result {
        Ok(()) => RouterOutcome::Result(Value::Null),
        Err(error) => RouterOutcome::Error(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_windows_returns_array() {
        let result = handle_method("list_windows", Value::Null);
        match result {
            RouterOutcome::Result(value) => assert!(value.is_array()),
            RouterOutcome::Error(error) => panic!("list_windows failed: {error}"),
            RouterOutcome::Close(_) => panic!("list_windows unexpectedly closed"),
        }
    }

    #[test]
    fn close_requests_shutdown() {
        assert_eq!(
            handle_method("close", Value::Null),
            RouterOutcome::Close(Value::Null)
        );
    }
}

