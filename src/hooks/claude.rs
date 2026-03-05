use std::collections::HashMap;

use super::context::{self, HookContext};
use super::{HookError, HookHandler, HookOutput};
use crate::{emit, parsers, sensitive};

// ---------------------------------------------------------------------------
// Static compaction context (injected on session-start)
// ---------------------------------------------------------------------------

const COMPACTION_CONTEXT: &str = "\
[Post-compaction context -- shepard-obs-stack]
- Metrics: shepherd_ prefix (OTel Collector Prometheus exporter namespace)
- Hook shell: set -u only (NOT set -euo pipefail -- SIGPIPE kills)
- Fire-and-forget: emit_counter uses curl -s & disown -- never block CLI
- Dashboards: edit JSON in configs/grafana/dashboards/ (UI edits lost on restart)
- PromQL: increase() returns floats -> wrap in round() for counters
- Empty model labels: filter with model!=\"\"
- Git identity: Shepard (digitalashes@users.noreply.github.com), GPG-signed
- Hook metrics: tool_calls, events, sensitive_file_access, compaction_events (all _total)
- Native OTel: dots->underscores (claude_code.cost_usage.USD -> shepherd_claude_code_cost_usage_USD_total)
- Loki: service_name=\"claude-code\" / \"codex_cli_rs\" / \"gemini-cli\"
- Session Timeline: Prometheus span-metrics (NOT Tempo local-blocks)
- PreToolUse guard active: blocks .env, credentials, .pem, .key, id_rsa, .aws/ etc.";

// ---------------------------------------------------------------------------
// PreToolUse
// ---------------------------------------------------------------------------

pub struct PreToolUse;

impl HookHandler for PreToolUse {
    fn provider(&self) -> &'static str {
        "claude"
    }
    fn hook_name(&self) -> &'static str {
        "pre-tool-use"
    }

    fn execute(&self, ctx: &HookContext) -> Result<HookOutput, HookError> {
        let tool_input = &ctx.input["tool_input"];

        if let Some(matched) = sensitive::check_sensitive_access(tool_input) {
            return Ok(HookOutput::Block(format!(
                "Blocked: access to sensitive file {matched}"
            )));
        }

        Ok(HookOutput::Silent)
    }
}

// ---------------------------------------------------------------------------
// PostToolUse
// ---------------------------------------------------------------------------

pub struct PostToolUse;

impl HookHandler for PostToolUse {
    fn provider(&self) -> &'static str {
        "claude"
    }
    fn hook_name(&self) -> &'static str {
        "post-tool-use"
    }

    fn execute(&self, ctx: &HookContext) -> Result<HookOutput, HookError> {
        let tool_name = ctx.input["tool_name"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        let git_repo = ctx.git.repo.clone();

        // Check sensitive access
        let tool_input = &ctx.input["tool_input"];
        if sensitive::check_sensitive_access(tool_input).is_some() {
            let mut labels = HashMap::new();
            labels.insert("source".into(), "claude-code".into());
            labels.insert("tool".into(), tool_name.clone());
            labels.insert("git_repo".into(), git_repo.clone());
            emit::metric("sensitive_file_access", 1.0, &labels);
        }

        // Detect error in tool_response
        let tool_response = ctx.input["tool_response"].as_str().unwrap_or("");
        let tool_status = if super::detect_tool_error(tool_response) {
            "error"
        } else {
            "success"
        };

        // Emit tool_calls counter
        let mut labels = HashMap::new();
        labels.insert("source".into(), "claude-code".into());
        labels.insert("tool".into(), tool_name);
        labels.insert("tool_status".into(), tool_status.into());
        labels.insert("git_repo".into(), git_repo.clone());
        emit::metric("tool_calls", 1.0, &labels);

        // Emit events counter
        let mut labels = HashMap::new();
        labels.insert("source".into(), "claude-code".into());
        labels.insert("event_type".into(), "tool_use".into());
        labels.insert("git_repo".into(), git_repo);
        emit::metric("events", 1.0, &labels);

        Ok(HookOutput::Silent)
    }
}

// ---------------------------------------------------------------------------
// Stop
// ---------------------------------------------------------------------------

pub struct Stop;

impl HookHandler for Stop {
    fn provider(&self) -> &'static str {
        "claude"
    }
    fn hook_name(&self) -> &'static str {
        "stop"
    }

    fn execute(&self, ctx: &HookContext) -> Result<HookOutput, HookError> {
        // Re-entry guard
        if ctx.input["stop_hook_active"].as_str() == Some("true")
            || ctx.input["stop_hook_active"].as_bool() == Some(true)
        {
            return Ok(HookOutput::Silent);
        }

        let git_repo = ctx.git.repo.clone();

        // Emit session_end event
        let mut labels = HashMap::new();
        labels.insert("source".into(), "claude-code".into());
        labels.insert("event_type".into(), "session_end".into());
        labels.insert("git_repo".into(), git_repo.clone());
        emit::metric("events", 1.0, &labels);

        // Find and process session file
        if let Some(session_file) = context::find_claude_session(&ctx.cwd, &ctx.session_id) {
            let path_str = session_file.to_string_lossy().to_string();

            // Count compaction events
            if let Ok(contents) = std::fs::read_to_string(&session_file) {
                let compaction_count = contents.matches("compact_boundary").count();
                if compaction_count > 0 {
                    let mut labels = HashMap::new();
                    labels.insert("source".into(), "claude-code".into());
                    labels.insert("git_repo".into(), git_repo);
                    emit::metric("compaction_events", compaction_count as f64, &labels);
                }
            }

            // Parse session and emit traces
            let spans = parsers::claude::parse_to_spans(&path_str);
            if !spans.is_empty() {
                emit::traces("claude-code-session", &spans);
            }
        }

        Ok(HookOutput::Silent)
    }
}

// ---------------------------------------------------------------------------
// SessionStart
// ---------------------------------------------------------------------------

pub struct SessionStart;

impl HookHandler for SessionStart {
    fn provider(&self) -> &'static str {
        "claude"
    }
    fn hook_name(&self) -> &'static str {
        "session-start"
    }

    fn execute(&self, _ctx: &HookContext) -> Result<HookOutput, HookError> {
        Ok(HookOutput::Stdout(COMPACTION_CONTEXT.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pre_tool_use_blocks_env_file() {
        let ctx = HookContext {
            input: json!({
                "tool_name": "Read",
                "tool_input": { "file_path": "/app/.env" }
            }),
            cwd: ".".into(),
            git: crate::git_context::GitContext {
                repo: String::new(),
                branch: "unknown".into(),
            },
            session_id: String::new(),
        };
        let result = PreToolUse.execute(&ctx).unwrap();
        assert!(matches!(result, HookOutput::Block(_)));
    }

    #[test]
    fn pre_tool_use_allows_normal_file() {
        let ctx = HookContext {
            input: json!({
                "tool_name": "Read",
                "tool_input": { "file_path": "/app/src/main.rs" }
            }),
            cwd: ".".into(),
            git: crate::git_context::GitContext {
                repo: String::new(),
                branch: "unknown".into(),
            },
            session_id: String::new(),
        };
        let result = PreToolUse.execute(&ctx).unwrap();
        assert!(matches!(result, HookOutput::Silent));
    }

    #[test]
    fn session_start_returns_static_text() {
        let ctx = HookContext {
            input: json!({}),
            cwd: ".".into(),
            git: crate::git_context::GitContext {
                repo: String::new(),
                branch: "unknown".into(),
            },
            session_id: String::new(),
        };
        let result = SessionStart.execute(&ctx).unwrap();
        match result {
            HookOutput::Stdout(text) => {
                assert!(text.contains("Post-compaction context"));
                assert!(text.contains("PreToolUse guard active"));
            }
            _ => panic!("expected Stdout"),
        }
    }
}
