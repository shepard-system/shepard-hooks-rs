use std::collections::HashMap;

use super::context::{self, HookContext};
use super::{HookError, HookHandler, HookOutput};
use crate::{emit, parsers};

// ---------------------------------------------------------------------------
// Notify
// ---------------------------------------------------------------------------

pub struct Notify;

impl HookHandler for Notify {
    fn provider(&self) -> &'static str {
        "codex"
    }
    fn hook_name(&self) -> &'static str {
        "notify"
    }

    fn execute(&self, ctx: &HookContext) -> Result<HookOutput, HookError> {
        // Only handle agent-turn-complete events
        let event_type = ctx.input["type"].as_str().unwrap_or("");
        if event_type != "agent-turn-complete" {
            return Ok(HookOutput::Silent);
        }

        let git_repo = ctx.git.repo.clone();

        // Emit turn_end event
        let mut labels = HashMap::new();
        labels.insert("source".into(), "codex".into());
        labels.insert("event_type".into(), "turn_end".into());
        labels.insert("git_repo".into(), git_repo);
        emit::metric("events", 1.0, &labels);

        // Find session file by thread-id and parse → emit traces
        let thread_id = ctx.input["thread-id"]
            .as_str()
            .unwrap_or("");

        if let Some(session_file) = context::find_codex_session(thread_id) {
            let path_str = session_file.to_string_lossy().to_string();
            let spans = parsers::codex::parse_to_spans(&path_str);
            if !spans.is_empty() {
                emit::traces("codex-session", &spans);
            }
        }

        Ok(HookOutput::Silent)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::context::HookContext;
    use serde_json::json;

    fn make_ctx(input: serde_json::Value) -> HookContext {
        HookContext {
            input,
            cwd: ".".into(),
            git: crate::git_context::GitContext {
                repo: "test-repo".into(),
                branch: "main".into(),
            },
            session_id: String::new(),
        }
    }

    #[test]
    fn notify_skips_non_turn_complete() {
        let ctx = make_ctx(json!({ "type": "other-event" }));
        let result = Notify.execute(&ctx).unwrap();
        assert!(matches!(result, HookOutput::Silent));
    }

    #[test]
    fn notify_processes_turn_complete() {
        let ctx = make_ctx(json!({
            "type": "agent-turn-complete",
            "thread-id": "nonexistent-thread"
        }));
        // Should not panic even though session file won't exist and emit will fail
        let result = Notify.execute(&ctx).unwrap();
        assert!(matches!(result, HookOutput::Silent));
    }
}
