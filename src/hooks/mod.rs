pub mod claude;
pub mod codex;
pub mod context;
pub mod gemini;

use std::collections::HashMap;
use std::fmt;
use std::sync::LazyLock;

use regex::Regex;

use self::context::HookContext;
use crate::{emit, sensitive};

// ---------------------------------------------------------------------------
// HookHandler trait
// ---------------------------------------------------------------------------

/// Each hook is a unit struct implementing this trait.
pub trait HookHandler {
    #[allow(dead_code)]
    fn provider(&self) -> &'static str;
    #[allow(dead_code)]
    fn hook_name(&self) -> &'static str;

    /// Execute the hook logic. May emit metrics, parse sessions, block access.
    fn execute(&self, ctx: &HookContext) -> Result<HookOutput, HookError>;
}

// ---------------------------------------------------------------------------
// HookOutput
// ---------------------------------------------------------------------------

pub enum HookOutput {
    /// Silent exit (claude hooks, codex)
    Silent,
    /// Print text to stdout (claude session-start)
    Stdout(String),
    /// Print JSON to stdout (gemini hooks require `{}`)
    Json(serde_json::Value),
    /// Block with exit code 2 (pre-tool-use guard)
    Block(String),
}

// ---------------------------------------------------------------------------
// HookError
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum HookError {
    /// Unknown provider/hook_name combo
    UnknownHook { provider: String, hook_name: String },
    /// JSON parse failure
    InvalidInput(String),
}

impl fmt::Display for HookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HookError::UnknownHook {
                provider,
                hook_name,
            } => {
                write!(f, "unknown hook: {provider}/{hook_name}")
            }
            HookError::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
        }
    }
}

impl std::error::Error for HookError {}

// ---------------------------------------------------------------------------
// Dispatch registry
// ---------------------------------------------------------------------------

pub fn dispatch(provider: &str, hook_name: &str) -> Result<&'static dyn HookHandler, HookError> {
    match (provider, hook_name) {
        ("claude", "pre-tool-use") => Ok(&claude::PreToolUse),
        ("claude", "post-tool-use") => Ok(&claude::PostToolUse),
        ("claude", "stop") => Ok(&claude::Stop),
        ("claude", "session-start") => Ok(&claude::SessionStart),
        ("codex", "notify") => Ok(&codex::Notify),
        ("gemini", "after-tool") => Ok(&gemini::AfterTool),
        ("gemini", "after-model") => Ok(&gemini::AfterModel),
        ("gemini", "after-agent") => Ok(&gemini::AfterAgent),
        ("gemini", "session-end") => Ok(&gemini::SessionEnd),
        _ => Err(HookError::UnknownHook {
            provider: provider.to_string(),
            hook_name: hook_name.to_string(),
        }),
    }
}

// ---------------------------------------------------------------------------
// Error pattern detection (shared by PostToolUse and AfterTool)
// ---------------------------------------------------------------------------

static ERROR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(^error|"error"|traceback|exit code [1-9]|command failed|FAILED|panic:)"#)
        .expect("ERROR_RE is a valid regex")
});

pub fn detect_tool_error(response: &str) -> bool {
    ERROR_RE.is_match(response)
}

// ---------------------------------------------------------------------------
// Shared tool-use metrics emitter (used by PostToolUse + AfterTool)
// ---------------------------------------------------------------------------

/// Emit tool_calls, events, and optionally sensitive_file_access metrics.
pub fn emit_tool_use_metrics(ctx: &HookContext, source: &str) {
    let tool_name = ctx.input["tool_name"]
        .as_str()
        .or_else(|| ctx.input["toolName"].as_str())
        .unwrap_or("unknown")
        .to_string();
    let git_repo = ctx.git.repo.clone();

    // Check sensitive access
    let tool_input = &ctx.input["tool_input"];
    if sensitive::check_sensitive_access(tool_input).is_some() {
        let mut labels = HashMap::new();
        labels.insert("source".into(), source.into());
        labels.insert("tool".into(), tool_name.clone());
        labels.insert("git_repo".into(), git_repo.clone());
        emit::metric("sensitive_file_access", 1.0, &labels);
    }

    // Detect error in tool_response
    let tool_response = ctx.input["tool_response"].as_str().unwrap_or("");
    let tool_status = if detect_tool_error(tool_response) {
        "error"
    } else {
        "success"
    };

    // Emit tool_calls counter
    let mut labels = HashMap::new();
    labels.insert("source".into(), source.into());
    labels.insert("tool".into(), tool_name);
    labels.insert("tool_status".into(), tool_status.into());
    labels.insert("git_repo".into(), git_repo.clone());
    emit::metric("tool_calls", 1.0, &labels);

    // Emit events counter
    let mut labels = HashMap::new();
    labels.insert("source".into(), source.into());
    labels.insert("event_type".into(), "tool_use".into());
    labels.insert("git_repo".into(), git_repo);
    emit::metric("events", 1.0, &labels);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_unknown_returns_error() {
        let result = dispatch("unknown", "hook");
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(matches!(err, HookError::UnknownHook { .. }));
    }

    #[test]
    fn dispatch_all_known_combos() {
        let combos = [
            ("claude", "pre-tool-use"),
            ("claude", "post-tool-use"),
            ("claude", "stop"),
            ("claude", "session-start"),
            ("codex", "notify"),
            ("gemini", "after-tool"),
            ("gemini", "after-model"),
            ("gemini", "after-agent"),
            ("gemini", "session-end"),
        ];
        for (provider, hook_name) in combos {
            let result = dispatch(provider, hook_name);
            assert!(
                result.is_ok(),
                "dispatch({provider}, {hook_name}) should succeed"
            );
            let handler = result.unwrap();
            assert_eq!(handler.provider(), provider);
            assert_eq!(handler.hook_name(), hook_name);
        }
    }

    #[test]
    fn detect_tool_error_matches_patterns() {
        assert!(detect_tool_error("error: file not found"));
        assert!(detect_tool_error("Error: something broke"));
        assert!(detect_tool_error(r#"{"error":"bad request"}"#));
        assert!(detect_tool_error("Traceback (most recent call last):"));
        assert!(detect_tool_error("exit code 1"));
        assert!(detect_tool_error("command failed with status 2"));
        assert!(detect_tool_error("FAILED to connect"));
        assert!(detect_tool_error("panic: runtime error"));
    }

    #[test]
    fn detect_tool_error_ignores_clean_responses() {
        assert!(!detect_tool_error("file contents here"));
        assert!(!detect_tool_error("exit code 0"));
        assert!(!detect_tool_error("Success"));
        assert!(!detect_tool_error(""));
    }
}
