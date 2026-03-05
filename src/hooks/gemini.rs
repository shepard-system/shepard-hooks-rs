use std::collections::HashMap;

use serde_json::json;

use super::context::{self, HookContext};
use super::{HookError, HookHandler, HookOutput};
use crate::{emit, parsers};

// ---------------------------------------------------------------------------
// AfterTool
// ---------------------------------------------------------------------------

pub struct AfterTool;

impl HookHandler for AfterTool {
    fn provider(&self) -> &'static str {
        "gemini"
    }
    fn hook_name(&self) -> &'static str {
        "after-tool"
    }

    fn execute(&self, ctx: &HookContext) -> Result<HookOutput, HookError> {
        super::emit_tool_use_metrics(ctx, "gemini-cli");
        Ok(HookOutput::Json(json!({})))
    }
}

// ---------------------------------------------------------------------------
// AfterModel
// ---------------------------------------------------------------------------

pub struct AfterModel;

impl HookHandler for AfterModel {
    fn provider(&self) -> &'static str {
        "gemini"
    }
    fn hook_name(&self) -> &'static str {
        "after-model"
    }

    fn execute(&self, ctx: &HookContext) -> Result<HookOutput, HookError> {
        // Only emit on final chunk (finishReason present)
        let finish_reason = ctx.input["llm_response"]["candidates"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|c| c["finishReason"].as_str());

        if finish_reason.is_none() {
            return Ok(HookOutput::Json(json!({})));
        }

        let git_repo = ctx.git.repo.clone();

        let mut labels = HashMap::new();
        labels.insert("source".into(), "gemini-cli".into());
        labels.insert("event_type".into(), "model_call".into());
        labels.insert("git_repo".into(), git_repo);
        emit::metric("events", 1.0, &labels);

        Ok(HookOutput::Json(json!({})))
    }
}

// ---------------------------------------------------------------------------
// AfterAgent
// ---------------------------------------------------------------------------

pub struct AfterAgent;

impl HookHandler for AfterAgent {
    fn provider(&self) -> &'static str {
        "gemini"
    }
    fn hook_name(&self) -> &'static str {
        "after-agent"
    }

    fn execute(&self, ctx: &HookContext) -> Result<HookOutput, HookError> {
        let git_repo = ctx.git.repo.clone();

        let mut labels = HashMap::new();
        labels.insert("source".into(), "gemini-cli".into());
        labels.insert("event_type".into(), "turn_end".into());
        labels.insert("git_repo".into(), git_repo);
        emit::metric("events", 1.0, &labels);

        Ok(HookOutput::Json(json!({})))
    }
}

// ---------------------------------------------------------------------------
// SessionEnd
// ---------------------------------------------------------------------------

pub struct SessionEnd;

impl HookHandler for SessionEnd {
    fn provider(&self) -> &'static str {
        "gemini"
    }
    fn hook_name(&self) -> &'static str {
        "session-end"
    }

    fn execute(&self, ctx: &HookContext) -> Result<HookOutput, HookError> {
        let git_repo = ctx.git.repo.clone();

        // Emit session_end event
        let mut labels = HashMap::new();
        labels.insert("source".into(), "gemini-cli".into());
        labels.insert("event_type".into(), "session_end".into());
        labels.insert("git_repo".into(), git_repo);
        emit::metric("events", 1.0, &labels);

        // Find session file by prefix match and parse → emit traces
        if let Some(session_file) = context::find_gemini_session(&ctx.session_id) {
            let path_str = session_file.to_string_lossy().to_string();
            let spans = parsers::gemini::parse_to_spans(&path_str);
            if !spans.is_empty() {
                emit::traces("gemini-session", &spans);
            }
        }

        Ok(HookOutput::Json(json!({})))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::context::HookContext;

    fn make_ctx(input: serde_json::Value) -> HookContext {
        HookContext {
            input,
            cwd: ".".into(),
            git: crate::git_context::GitContext {
                repo: "test-repo".into(),
            },
            session_id: String::new(),
        }
    }

    #[test]
    fn after_tool_returns_json() {
        let ctx = make_ctx(json!({
            "tool_name": "read_file",
            "tool_input": { "file_path": "/app/main.py" },
            "tool_response": "contents"
        }));
        let result = AfterTool.execute(&ctx).unwrap();
        match result {
            HookOutput::Json(v) => assert_eq!(v, json!({})),
            _ => panic!("expected Json"),
        }
    }

    #[test]
    fn after_model_returns_json_without_finish_reason() {
        let ctx = make_ctx(json!({
            "llm_response": { "candidates": [{}] }
        }));
        let result = AfterModel.execute(&ctx).unwrap();
        match result {
            HookOutput::Json(v) => assert_eq!(v, json!({})),
            _ => panic!("expected Json"),
        }
    }

    #[test]
    fn session_end_returns_json() {
        let ctx = make_ctx(json!({}));
        let result = SessionEnd.execute(&ctx).unwrap();
        match result {
            HookOutput::Json(v) => assert_eq!(v, json!({})),
            _ => panic!("expected Json"),
        }
    }
}
