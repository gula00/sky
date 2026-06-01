use serde_json::Value;
use std::time::{Duration, Instant};

pub const REQUEST_BUDGET_META_KEY: &str = "x-oai-cua-request-budget-ms";

#[derive(Debug, Clone)]
pub struct RequestBudget {
    deadline: Instant,
}

impl RequestBudget {
    pub fn from_meta(meta: &Value) -> Result<Option<Self>, String> {
        let Some(ms) = request_budget_ms(meta)? else {
            return Ok(None);
        };

        let now = Instant::now();
        let duration = Duration::from_millis(ms);
        Ok(Some(Self {
            deadline: now.checked_add(duration).unwrap_or(now),
        }))
    }

    pub fn check(&self, method: &str) -> Result<(), String> {
        if Instant::now() >= self.deadline {
            Err(format!("computer-use request timed out: {method}"))
        } else {
            Ok(())
        }
    }

    pub fn deadline(&self) -> Instant {
        self.deadline
    }
}

pub fn check_request_budget(meta: &Value, method: &str) -> Result<Option<RequestBudget>, String> {
    let budget = RequestBudget::from_meta(meta)?;
    if let Some(budget) = &budget {
        budget.check(method)?;
    }
    Ok(budget)
}

fn request_budget_ms(meta: &Value) -> Result<Option<u64>, String> {
    metadata_candidates(meta)
        .into_iter()
        .find_map(|candidate| candidate.get(REQUEST_BUDGET_META_KEY))
        .map(parse_budget_value)
        .transpose()
}

fn parse_budget_value(value: &Value) -> Result<u64, String> {
    let ms = match value {
        Value::Number(number) => number
            .as_u64()
            .ok_or_else(|| "request budget must be a non-negative integer".to_string())?,
        Value::String(value) => value
            .trim()
            .parse::<u64>()
            .map_err(|_| "request budget must be a non-negative integer".to_string())?,
        _ => return Err("request budget must be a number or numeric string".to_string()),
    };

    if ms == 0 {
        Err("request budget must be greater than 0".to_string())
    } else {
        Ok(ms)
    }
}

fn metadata_candidates(metadata: &Value) -> Vec<&Value> {
    let mut candidates = vec![metadata];
    if let Some(nested) = metadata.get("x-codex-turn-metadata") {
        candidates.push(nested);
    }
    if let Some(nested) = metadata.get("codexTurnMetadata") {
        candidates.push(nested);
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{thread, time::Duration};

    #[test]
    fn parses_direct_budget() {
        let budget = RequestBudget::from_meta(&json!({
            REQUEST_BUDGET_META_KEY: 1000
        }))
        .unwrap();

        assert!(budget.is_some());
    }

    #[test]
    fn parses_nested_budget() {
        let budget = RequestBudget::from_meta(&json!({
            "codexTurnMetadata": {
                REQUEST_BUDGET_META_KEY: "1000"
            }
        }))
        .unwrap();

        assert!(budget.is_some());
    }

    #[test]
    fn rejects_zero_budget() {
        let error = RequestBudget::from_meta(&json!({
            REQUEST_BUDGET_META_KEY: 0
        }))
        .unwrap_err();

        assert!(error.contains("greater than 0"));
    }

    #[test]
    fn reports_elapsed_budget() {
        let budget = RequestBudget::from_meta(&json!({
            REQUEST_BUDGET_META_KEY: 1
        }))
        .unwrap()
        .unwrap();

        thread::sleep(Duration::from_millis(3));
        let error = budget.check("list_windows").unwrap_err();
        assert!(error.contains("timed out"));
    }
}
