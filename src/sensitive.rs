use regex::Regex;
use std::sync::LazyLock;

/// File path patterns — checked against file_path and notebook_path.
static FILE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(\.env$|\.env\.|credentials|secrets|\.pem$|\.key$|id_rsa|id_ed25519|\.p12$|password|token\.json|\.secret|\.aws/)").expect("FILE_RE is a valid regex")
});

/// Command patterns — more specific to avoid false positives.
static CMD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(\.env\s|\.env$|/\.env|credentials\.|credentials/|/secrets/|/secrets$|\.pem\s|\.pem$|\.key\s|\.key$|id_rsa|id_ed25519|\.p12\s|\.p12$|token\.json|\.secret|\.aws/)").expect("CMD_RE is a valid regex")
});

/// Check if a file path matches sensitive patterns.
pub fn is_sensitive_path(path: &str) -> bool {
    !path.is_empty() && FILE_RE.is_match(path)
}

/// Check if a command matches sensitive patterns.
pub fn is_sensitive_command(cmd: &str) -> bool {
    !cmd.is_empty() && CMD_RE.is_match(cmd)
}

/// Check tool input JSON for sensitive access. Returns the matched string if found.
pub fn check_sensitive_access(tool_input: &serde_json::Value) -> Option<String> {
    let file_path = tool_input["file_path"]
        .as_str()
        .or_else(|| tool_input["notebook_path"].as_str())
        .unwrap_or("");

    if is_sensitive_path(file_path) {
        return Some(file_path.to_string());
    }

    let command = tool_input["command"].as_str().unwrap_or("");
    if is_sensitive_command(command) {
        return Some(command.to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_env_file() {
        assert!(is_sensitive_path("/app/.env"));
        assert!(is_sensitive_path("/app/.env.local"));
        assert!(is_sensitive_path("/app/credentials.json"));
    }

    #[test]
    fn ignores_normal_files() {
        assert!(!is_sensitive_path("/app/src/main.rs"));
        assert!(!is_sensitive_path("/app/environment.ts"));
    }

    #[test]
    fn avoids_command_false_positives() {
        assert!(!is_sensitive_command("aws configure export-credentials"));
        assert!(is_sensitive_command("cat /app/.env"));
        assert!(is_sensitive_command("cat credentials.json"));
    }
}
