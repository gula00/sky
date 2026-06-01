use serde_json::Value;
use std::path::{Path, PathBuf};

pub const STOPPED_BY_USER_MESSAGE: &str = "Computer Use was stopped by the user with the physical Escape key. Stop your work, do not call further Computer Use tools in this turn, and send a final message noting that the user stopped Computer Use.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnScope {
    pub codex_home: PathBuf,
    pub session_id: String,
    pub turn_id: String,
}

#[derive(Debug, Default)]
pub struct ActiveTurnState {
    active_key: Option<String>,
}

impl ActiveTurnState {
    pub fn observe_request(&mut self, method: &str, metadata: &Value) {
        if matches!(method, "close" | "end_turn") {
            self.active_key = None;
            return;
        }

        if let Some(key) = key_from_metadata(metadata) {
            self.active_key = Some(key);
        }
    }

    #[cfg(test)]
    pub fn active_key(&self) -> Option<&str> {
        self.active_key.as_deref()
    }
}

impl TurnScope {
    pub fn interrupt_path(&self) -> PathBuf {
        interrupt_path(&self.codex_home, &self.session_id, &self.turn_id)
    }
}

pub fn scope_from_metadata(metadata: &Value) -> Option<TurnScope> {
    metadata_candidates(metadata)
        .into_iter()
        .find_map(scope_from_metadata_object)
}

pub fn is_interrupted(metadata: &Value) -> bool {
    scope_from_metadata(metadata)
        .map(|scope| scope.interrupt_path().exists())
        .unwrap_or(false)
}

pub fn key_from_metadata(metadata: &Value) -> Option<String> {
    scope_from_metadata(metadata).map(|scope| {
        format!(
            "{}\0{}\0{}",
            scope.codex_home.display(),
            scope.session_id,
            scope.turn_id
        )
    })
}

pub fn method_can_bypass_interrupt(method: &str) -> bool {
    matches!(method, "close" | "end_turn")
}

pub fn write_interrupt_file(scope: &TurnScope) -> std::io::Result<()> {
    let path = scope.interrupt_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, "")
}

pub fn interrupt_path(codex_home: &Path, session_id: &str, turn_id: &str) -> PathBuf {
    codex_home
        .join("cache")
        .join("computer-use")
        .join("interrupts")
        .join(sanitize_path_part(session_id))
        .join(sanitize_path_part(turn_id))
}

pub fn sanitize_path_part(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '_' | '-' => ch,
            _ => '_',
        })
        .collect()
}

fn scope_from_metadata_object(value: &Value) -> Option<TurnScope> {
    let object = value.as_object()?;
    let session_id = first_string(value, &["session_id", "conversation_id", "conversationId"])?;
    let turn_id = first_string(value, &["turn_id", "turnId"])?;
    let codex_home = first_string(value, &["codex_home", "codexHome"])
        .map(PathBuf::from)
        .or_else(default_codex_home)?;

    if object.is_empty() {
        None
    } else {
        Some(TurnScope {
            codex_home,
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
        })
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

fn first_string<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn default_codex_home() -> Option<PathBuf> {
    std::env::var("CODEX_HOME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("USERPROFILE")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".codex"))
        })
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".codex"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_top_level_metadata() {
        let scope = scope_from_metadata(&json!({
            "codexHome": r"C:\codex-home",
            "session_id": "session/1",
            "turn_id": "turn:2"
        }))
        .unwrap();

        assert_eq!(scope.codex_home, PathBuf::from(r"C:\codex-home"));
        assert_eq!(scope.session_id, "session/1");
        assert_eq!(scope.turn_id, "turn:2");
    }

    #[test]
    fn parses_nested_metadata() {
        let scope = scope_from_metadata(&json!({
            "x-codex-turn-metadata": {
                "codexHome": r"C:\codex-home",
                "conversation_id": "conversation",
                "turn_id": "turn"
            }
        }))
        .unwrap();

        assert_eq!(scope.session_id, "conversation");
        assert_eq!(scope.turn_id, "turn");
    }

    #[test]
    fn interrupt_path_sanitizes_parts() {
        assert_eq!(
            interrupt_path(Path::new(r"C:\codex"), "session/1", "turn:2"),
            PathBuf::from(r"C:\codex")
                .join("cache")
                .join("computer-use")
                .join("interrupts")
                .join("session_1")
                .join("turn_2")
        );
    }

    #[test]
    fn active_turn_state_tracks_and_clears_turns() {
        let mut state = ActiveTurnState::default();
        state.observe_request(
            "list_windows",
            &json!({
                "codexHome": r"C:\codex-home",
                "session_id": "session",
                "turn_id": "turn"
            }),
        );

        assert_eq!(
            state.active_key(),
            Some("C:\\codex-home\0session\0turn")
        );

        state.observe_request(
            "end_turn",
            &json!({
                "codexHome": r"C:\codex-home",
                "session_id": "session",
                "turn_id": "turn"
            }),
        );
        assert_eq!(state.active_key(), None);
    }
}
