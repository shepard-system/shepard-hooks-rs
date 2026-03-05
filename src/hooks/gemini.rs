use std::collections::HashMap;

use serde_json::json;

use super::context::{self, HookContext};
use super::{HookError, HookHandler, HookOutput};
use crate::{emit, parsers, sensitive};

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
            labels.insert("source".into(), "gemini-cli".into());
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
        labels.insert("source".into(), "gemini-cli".into());
        labels.insert("tool".into(), tool_name);
        labels.insert("tool_status".into(), tool_status.into());
        labels.insert("git_repo".into(), git_repo.clone());
        emit::metric("tool_calls", 1.0, &labels);

        // Emit events counter
        let mut labels = HashMap::new();
        labels.insert("source".into(), "gemini-cli".into());
        labels.insert("event_type".into(), "tool_use".into());
        labels.insert("git_repo".into(), git_repo);
        emit::metric("events", 1.0, &labels);

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
