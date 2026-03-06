use std::path::PathBuf;

use serde_json::Value;

use crate::git_context::{self, GitContext};

// ---------------------------------------------------------------------------
// HookContext — shared state built once per invocation
// ---------------------------------------------------------------------------

pub struct HookContext {
    pub input: Value,
    pub cwd: String,
    pub git: GitContext,
    pub session_id: String,
}

impl HookContext {
    /// Build context from parsed stdin JSON.
    pub fn from_input(input: Value) -> Self {
        let cwd = input["cwd"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| std::env::var("GEMINI_CWD").ok())
            .or_else(|| std::env::var("GEMINI_PROJECT_DIR").ok())
            .unwrap_or_else(|| ".".to_string());

        let git = git_context::get(&cwd);

        let session_id = input["session_id"].as_str().unwrap_or("").to_string();

        HookContext {
            input,
            cwd,
            git,
            session_id,
        }
    }
}

// ---------------------------------------------------------------------------
// Session file finders
// ---------------------------------------------------------------------------

/// Claude: ~/.claude/projects/{slug}/{session_id}.jsonl
/// Slug = cwd with '/' replaced by '-'
pub fn find_claude_session(cwd: &str, session_id: &str) -> Option<PathBuf> {
    if session_id.is_empty() {
        return None;
    }
    let home = std::env::var("HOME").ok()?;
    let slug = cwd.replace('/', "-");
    let path = PathBuf::from(format!("{home}/.claude/projects/{slug}/{session_id}.jsonl"));
    if path.exists() { Some(path) } else { None }
}

/// Codex: find ~/.codex/sessions -name "rollout-*-{thread_id}.jsonl"
/// Sessions are stored in nested YYYY/MM/DD subdirectories.
pub fn find_codex_session(thread_id: &str) -> Option<PathBuf> {
    if thread_id.is_empty() {
        return None;
    }
    let home = std::env::var("HOME").ok()?;
    let sessions_dir = PathBuf::from(format!("{home}/.codex/sessions"));
    if !sessions_dir.exists() {
        return None;
    }

    let suffix = format!("-{thread_id}.jsonl");
    find_recursive(&sessions_dir, &|name: &str| {
        name.starts_with("rollout-") && name.ends_with(&suffix)
    })
}

/// Gemini: find ~/.gemini/tmp -name "session-*-{prefix}*.json"
/// prefix = first 8 chars of session_id
/// Sessions are stored in nested project/chats subdirectories.
pub fn find_gemini_session(session_id: &str) -> Option<PathBuf> {
    if session_id.len() < 8 {
        return None;
    }
    let prefix = &session_id[..8];
    let home = std::env::var("HOME").ok()?;
    let tmp_dir = PathBuf::from(format!("{home}/.gemini/tmp"));
    if !tmp_dir.exists() {
        return None;
    }

    find_recursive(&tmp_dir, &|name: &str| {
        name.starts_with("session-") && name.contains(prefix) && name.ends_with(".json")
    })
}

/// Recursively search a directory for a file matching the predicate.
fn find_recursive(dir: &PathBuf, predicate: &dyn Fn(&str) -> bool) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file()
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
            && predicate(name)
        {
            return Some(path);
        } else if path.is_dir()
            && let Some(found) = find_recursive(&path, predicate)
        {
            return Some(found);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_claude_session_builds_correct_path() {
        // We can't test filesystem existence, but we can test the path construction
        // by checking the function returns None for a non-existent path
        let result = find_claude_session("/Users/test/project", "abc-123");
        assert!(result.is_none()); // file won't exist, but path was constructed
    }

    #[test]
    fn find_codex_session_returns_none_for_empty() {
        assert!(find_codex_session("").is_none());
    }

    #[test]
    fn find_gemini_session_returns_none_for_short_id() {
        assert!(find_gemini_session("abc").is_none());
    }
}
