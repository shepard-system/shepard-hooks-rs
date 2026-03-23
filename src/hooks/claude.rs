use std::collections::HashMap;

use super::context::{self, HookContext};
use super::{HookError, HookHandler, HookOutput};
use crate::{emit, parsers};

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
        // Only check file_path — command checking intentionally skipped to match
        // bash parity and avoid false positives (e.g. "aws configure export-credentials").
        // PostToolUse still counts command-based sensitive access via metrics.
        let file_path = ctx.input["tool_input"]["file_path"]
            .as_str()
            .or_else(|| ctx.input["tool_input"]["notebook_path"].as_str())
            .unwrap_or("");

        if crate::sensitive::is_sensitive_path(file_path) {
            return Ok(HookOutput::Block(format!(
                "Blocked: access to sensitive file {file_path}"
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
        super::emit_tool_use_metrics(ctx, "claude-code");
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
                    labels.insert("git_repo".into(), git_repo.clone());
                    emit::metric("compaction_events", compaction_count as f64, &labels);
                }
            }

            // Parse session and emit traces + context metrics
            let spans = parsers::claude::parse_to_spans(&path_str);
            if !spans.is_empty() {
                emit::traces("claude-code-session", &spans);

                // Extract context breakdown from root span (first span)
                // and emit as metrics — matches bash session-parser logic
                let root_attrs = &spans[0]["attributes"];

                // context_chars metric (by type) — only when > 0
                for (attr_key, type_label) in [
                    ("context.tool_output_chars", "tool_output"),
                    ("context.user_prompt_chars", "user_prompt"),
                    ("context.compact_summary_chars", "compact_summary"),
                ] {
                    let val: f64 = root_attrs[attr_key]
                        .as_str()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0.0);
                    if val > 0.0 {
                        let mut labels = HashMap::new();
                        labels.insert("source".into(), "claude-code".into());
                        labels.insert("type".into(), type_label.into());
                        labels.insert("git_repo".into(), git_repo.clone());
                        emit::metric("context_chars", val, &labels);
                    }
                }

                // context_compaction_pre_tokens metric — only when > 0
                let pre_tokens: f64 = root_attrs["context.compaction_pre_tokens"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);
                if pre_tokens > 0.0 {
                    let mut labels = HashMap::new();
                    labels.insert("source".into(), "claude-code".into());
                    labels.insert("git_repo".into(), git_repo.clone());
                    emit::metric("context_compaction_pre_tokens", pre_tokens, &labels);
                }
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
            },
            session_id: String::new(),
        };
        let result = PreToolUse.execute(&ctx).unwrap();
        assert!(matches!(result, HookOutput::Silent));
    }

    #[test]
    fn pre_tool_use_allows_sensitive_command() {
        // Commands are intentionally NOT blocked in PreToolUse (bash parity)
        let ctx = HookContext {
            input: json!({
                "tool_name": "Bash",
                "tool_input": { "command": "cat /app/.env" }
            }),
            cwd: ".".into(),
            git: crate::git_context::GitContext {
                repo: String::new(),
            },
            session_id: String::new(),
        };
        let result = PreToolUse.execute(&ctx).unwrap();
        assert!(matches!(result, HookOutput::Silent));
    }

    #[test]
    fn post_tool_use_returns_silent_on_normal_tool() {
        let ctx = HookContext {
            input: json!({
                "tool_name": "Read",
                "tool_input": { "file_path": "/app/src/main.rs" },
                "tool_response": "file contents here"
            }),
            cwd: ".".into(),
            git: crate::git_context::GitContext {
                repo: "test-repo".into(),
            },
            session_id: String::new(),
        };
        let result = PostToolUse.execute(&ctx).unwrap();
        assert!(matches!(result, HookOutput::Silent));
    }

    #[test]
    fn post_tool_use_returns_silent_on_error_response() {
        let ctx = HookContext {
            input: json!({
                "tool_name": "Bash",
                "tool_input": { "command": "make build" },
                "tool_response": "error: compilation failed\nexit code 1"
            }),
            cwd: ".".into(),
            git: crate::git_context::GitContext {
                repo: "test-repo".into(),
            },
            session_id: String::new(),
        };
        let result = PostToolUse.execute(&ctx).unwrap();
        assert!(matches!(result, HookOutput::Silent));
    }

    #[test]
    fn stop_returns_silent_when_active() {
        // stop_hook_active=true → re-entry guard → Silent
        let ctx = HookContext {
            input: json!({ "stop_hook_active": "true" }),
            cwd: ".".into(),
            git: crate::git_context::GitContext {
                repo: "test-repo".into(),
            },
            session_id: String::new(),
        };
        let result = Stop.execute(&ctx).unwrap();
        assert!(matches!(result, HookOutput::Silent));
    }

    #[test]
    fn stop_returns_silent_when_not_active() {
        // stop_hook_active absent, no session file → still Silent
        let ctx = HookContext {
            input: json!({}),
            cwd: ".".into(),
            git: crate::git_context::GitContext {
                repo: "test-repo".into(),
            },
            session_id: "nonexistent-session-id".into(),
        };
        let result = Stop.execute(&ctx).unwrap();
        assert!(matches!(result, HookOutput::Silent));
    }

    #[test]
    fn session_start_returns_static_text() {
        let ctx = HookContext {
            input: json!({}),
            cwd: ".".into(),
            git: crate::git_context::GitContext {
                repo: String::new(),
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
