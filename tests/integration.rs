use std::process::Command;

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_shepard-hook"))
}

// ---------------------------------------------------------------------------
// parse-session integration tests
// ---------------------------------------------------------------------------

#[test]
fn parse_session_claude_produces_spans() {
    let output = binary()
        .args(["parse-session", "claude", "tests/fixtures/claude-session.jsonl"])
        .output()
        .expect("failed to run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    // Root span + meta span + 1 tool span = 3
    assert_eq!(lines.len(), 3, "expected 3 spans, got {}: {stdout}", lines.len());

    // Verify root span
    let root: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(root["name"], "claude.session");
    assert_eq!(root["attributes"]["provider"], "claude-code");
    assert_eq!(root["attributes"]["model"], "claude-sonnet-4-20250514");
    assert_eq!(root["attributes"]["tokens.input"], "220");
    assert_eq!(root["attributes"]["tokens.output"], "80");
    assert_eq!(root["attributes"]["tool.count"], "1");
    assert_eq!(root["attributes"]["stop_reason"], "end_turn");

    // Verify trace_id is session ID without dashes
    assert_eq!(root["trace_id"], "aaaaaaaabbbbccccddddeeeeeeeeeeee");

    // Verify tool span
    let tool: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
    assert_eq!(tool["name"], "claude.tool.Read");
    assert_eq!(tool["attributes"]["tool.name"], "Read");
    assert_eq!(tool["attributes"]["tool.input.file_path"], "/app/src/main.rs");
}

#[test]
fn parse_session_codex_produces_spans() {
    let output = binary()
        .args(["parse-session", "codex", "tests/fixtures/codex-session.jsonl"])
        .output()
        .expect("failed to run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    // Root + meta + 1 tool = 3
    assert_eq!(lines.len(), 3, "expected 3 spans, got {}: {stdout}", lines.len());

    let root: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(root["name"], "codex.session");
    assert_eq!(root["attributes"]["model"], "o3-mini");
    assert_eq!(root["attributes"]["git.branch"], "main");
    assert_eq!(root["attributes"]["git.repo"], "repo");
    assert_eq!(root["attributes"]["tokens.input"], "200");
    assert_eq!(root["attributes"]["tokens.output"], "100");
    assert_eq!(root["attributes"]["tokens.reasoning"], "30");
    assert_eq!(root["attributes"]["stop_reason"], "end_turn");

    let tool: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
    assert_eq!(tool["name"], "codex.tool.shell");
    assert_eq!(tool["attributes"]["tool.input.command"], "ls -la");
}

#[test]
fn parse_session_gemini_produces_spans() {
    let output = binary()
        .args(["parse-session", "gemini", "tests/fixtures/gemini-session.json"])
        .output()
        .expect("failed to run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    // Root + meta + 1 tool = 3
    assert_eq!(lines.len(), 3, "expected 3 spans, got {}: {stdout}", lines.len());

    let root: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(root["name"], "gemini.session");
    assert_eq!(root["attributes"]["model"], "gemini-2.5-pro");
    assert_eq!(root["attributes"]["tokens.input"], "50");
    assert_eq!(root["attributes"]["tokens.output"], "30");
    assert_eq!(root["attributes"]["tokens.reasoning"], "10");
    assert_eq!(root["attributes"]["turn.count"], "1");

    let tool: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
    assert_eq!(tool["name"], "gemini.tool.read_file");
    assert_eq!(tool["attributes"]["tool.input.file_path"], "/app/main.py");
}

#[test]
fn parse_session_unknown_provider_fails() {
    let output = binary()
        .args(["parse-session", "unknown", "fake.jsonl"])
        .output()
        .expect("failed to run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown provider"));
}

// ---------------------------------------------------------------------------
// hook command integration tests
// ---------------------------------------------------------------------------

#[test]
fn hook_claude_pre_tool_use_blocks_env() {
    let output = binary()
        .args(["hook", "claude", "pre-tool-use"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(br#"{"tool_name":"Read","tool_input":{"file_path":"/app/.env"}}"#)
                .unwrap();
            child.wait_with_output()
        })
        .expect("failed to run");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Blocked"));
    assert!(stderr.contains(".env"));
}

#[test]
fn hook_claude_pre_tool_use_allows_normal() {
    let output = binary()
        .args(["hook", "claude", "pre-tool-use"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(br#"{"tool_name":"Read","tool_input":{"file_path":"/app/main.rs"}}"#)
                .unwrap();
            child.wait_with_output()
        })
        .expect("failed to run");

    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn hook_claude_session_start_outputs_context() {
    let output = binary()
        .args(["hook", "claude", "session-start"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(b"{}").unwrap();
            child.wait_with_output()
        })
        .expect("failed to run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Post-compaction context"));
    assert!(stdout.contains("PreToolUse guard active"));
}

#[test]
fn hook_gemini_after_agent_returns_json() {
    let output = binary()
        .args(["hook", "gemini", "after-agent"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(b"{}").unwrap();
            child.wait_with_output()
        })
        .expect("failed to run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "{}");
}

#[test]
fn hook_unknown_combo_fails() {
    let output = binary()
        .args(["hook", "unknown", "whatever"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(b"{}").unwrap();
            child.wait_with_output()
        })
        .expect("failed to run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown hook"));
}

#[test]
fn hook_codex_notify_ignores_non_turn_complete() {
    let output = binary()
        .args(["hook", "codex", "notify"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(br#"{"type":"other-event"}"#)
                .unwrap();
            child.wait_with_output()
        })
        .expect("failed to run");

    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn hook_gemini_after_model_skips_without_finish_reason() {
    let output = binary()
        .args(["hook", "gemini", "after-model"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(br#"{"llm_response":{"candidates":[{}]}}"#)
                .unwrap();
            child.wait_with_output()
        })
        .expect("failed to run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "{}");
}
