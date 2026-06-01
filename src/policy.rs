use anyhow::Result;

pub const ALLOW_FORBIDDEN_TARGETS_ENV: &str = "ComputerUseAllowForbiddenTargets";

pub fn ensure_target_allowed(app: &str, title: &str) -> Result<()> {
    if allow_forbidden_targets() {
        return Ok(());
    }

    if let Some(reason) = forbidden_target_reason(app, title) {
        anyhow::bail!("Computer Use cannot target {title:?}: {reason}");
    }
    Ok(())
}

pub fn ensure_app_allowed(app: &str) -> Result<()> {
    if allow_forbidden_targets() {
        return Ok(());
    }

    if let Some(reason) = forbidden_target_reason(app, "") {
        anyhow::bail!("Computer Use cannot target app {app:?}: {reason}");
    }
    Ok(())
}

fn allow_forbidden_targets() -> bool {
    allow_forbidden_targets_value(std::env::var(ALLOW_FORBIDDEN_TARGETS_ENV).ok().as_deref())
}

fn allow_forbidden_targets_value(value: Option<&str>) -> bool {
    matches!(
        value.map(|value| value.trim().to_ascii_lowercase()),
        Some(value) if matches!(value.as_str(), "1" | "true" | "yes" | "on" | "*")
    )
}

fn forbidden_target_reason(app: &str, title: &str) -> Option<String> {
    let process = process_stem(app);
    let process_lower = process.to_ascii_lowercase();
    let title_lower = title.trim().to_ascii_lowercase();

    if matches!(
        process_lower.as_str(),
        "consent"
            | "credentialuibroker"
            | "logonui"
            | "lockapp"
            | "lsass"
            | "securityhealthsystray"
            | "sethc"
            | "taskmgr"
            | "utilman"
            | "winlogon"
    ) {
        return Some(format!("forbidden Windows security target ({process})"));
    }

    if title_lower.contains("windows security")
        || title_lower.contains("credential")
        || title_lower.contains("user account control")
    {
        return Some("forbidden Windows security surface".to_string());
    }

    None
}

fn process_stem(app: &str) -> String {
    let value = app.strip_prefix("process:").unwrap_or(app);
    let normalized = value.replace('\\', "/");
    let file_name = normalized.rsplit('/').next().unwrap_or(value);
    file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_forbidden_security_processes() {
        assert!(forbidden_target_reason(r"process:C:\Windows\System32\consent.exe", "").is_some());
        assert!(
            forbidden_target_reason(r"process:C:\Windows\System32\CredentialUIBroker.exe", "")
                .is_some()
        );
    }

    #[test]
    fn recognizes_forbidden_security_titles() {
        assert!(
            forbidden_target_reason(r"process:C:\Windows\System32\notepad.exe", "Windows Security")
                .is_some()
        );
    }

    #[test]
    fn accepts_normal_processes() {
        assert!(
            forbidden_target_reason(r"process:C:\Windows\System32\notepad.exe", "notes").is_none()
        );
    }

    #[test]
    fn parses_allow_override_values() {
        assert!(allow_forbidden_targets_value(Some("true")));
        assert!(allow_forbidden_targets_value(Some("1")));
        assert!(allow_forbidden_targets_value(Some("*")));
        assert!(!allow_forbidden_targets_value(Some("false")));
        assert!(!allow_forbidden_targets_value(None));
    }
}
