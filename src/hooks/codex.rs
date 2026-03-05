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
