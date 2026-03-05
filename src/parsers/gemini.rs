use serde_json::{json, Value};
use std::error::Error;
use std::fs;

use super::common::{pad16, ts_to_ns};

/// Parse a Gemini CLI JSON session file and return spans as Vec<Value>.
pub fn parse_to_spans(file_path: &str) -> Vec<Value> {
    parse_inner(file_path).unwrap_or_default()
}

/// Parse a Gemini CLI JSON session file and emit span JSONL to stdout.
/// Note: Gemini uses a single JSON file (not JSONL).
pub fn parse(file_path: &str) -> Result<(), Box<dyn Error>> {
    let spans = parse_inner(file_path)?;
    for span in &spans {
        println!("{}", serde_json::to_string(span).unwrap());
    }
    Ok(())
}

fn parse_inner(file_path: &str) -> Result<Vec<Value>, Box<dyn Error>> {
    let mut spans = Vec::new();
    let content = fs::read_to_string(file_path)?;
    let session: Value = serde_json::from_str(&content)?;

    let session_id = session["sessionId"].as_str().unwrap_or("");
    if session_id.is_empty() {
        return Ok(spans);
    }

    let trace_id = session_id.replace('-', "");
    let root_sid = "0000000000000001";
    let meta_sid = "0000000000000002";

    let t_start = session["startTime"].as_str().unwrap_or("");
    let t_end = session["lastUpdated"].as_str().unwrap_or("");

    let msgs = session["messages"].as_array();
    let empty_vec = vec![];
    let msgs = msgs.unwrap_or(&empty_vec);

    // Model from first gemini message
    let model = msgs
        .iter()
        .find_map(|m| {
            if m["type"].as_str() == Some("gemini") {
                m["model"].as_str()
            } else {
                None
            }
        })
        .unwrap_or("unknown");

    // Token aggregation
    let (mut tok_in, mut tok_out, mut tok_cached, mut tok_thoughts, mut tok_total) =
        (0i64, 0i64, 0i64, 0i64, 0i64);
    for m in msgs {
        if m["type"].as_str() == Some("gemini") {
            let t = &m["tokens"];
            tok_in += t["input"].as_i64().unwrap_or(0);
            tok_out += t["output"].as_i64().unwrap_or(0);
            tok_cached += t["cached"].as_i64().unwrap_or(0);
            tok_thoughts += t["thoughts"].as_i64().unwrap_or(0);
            tok_total += t["total"].as_i64().unwrap_or(0);
        }
    }

    // Flatten tool calls with parent message context
    struct ToolEntry {
        name: String,
        status: String,
        timestamp: String,
        msg_ts: String,
        args: Value,
    }

    let mut all_tools: Vec<ToolEntry> = Vec::new();
    for m in msgs {
        if m["type"].as_str() != Some("gemini") {
            continue;
        }
        let msg_ts = m["timestamp"].as_str().unwrap_or("");
        if let Some(tool_calls) = m["toolCalls"].as_array() {
            for tc in tool_calls {
                all_tools.push(ToolEntry {
                    name: tc["name"].as_str().unwrap_or("").to_string(),
                    status: tc["status"].as_str().unwrap_or("").to_string(),
                    timestamp: tc["timestamp"].as_str().unwrap_or(msg_ts).to_string(),
                    msg_ts: msg_ts.to_string(),
                    args: tc["args"].clone(),
                });
            }
        }
    }

    let tool_error_count = all_tools
        .iter()
        .filter(|t| t.status == "error" || t.status == "cancelled")
        .count();

    // Turn count (user messages)
    let turn_count = msgs.iter().filter(|m| m["type"].as_str() == Some("user")).count();

    // Thinking blocks
    let thinking_count: usize = msgs
        .iter()
        .filter(|m| m["type"].as_str() == Some("gemini"))
        .map(|m| m["thoughts"].as_array().map(|a| a.len()).unwrap_or(0))
        .sum();

    // Interruptions
    let interruption_count = msgs
        .iter()
        .filter(|m| {
            m["type"].as_str() == Some("info")
                && m["content"].as_str() == Some("Request cancelled.")
        })
        .count();

    // ===== Emit spans =====

    let mut root_attrs = json!({
        "session.id": session_id, "model": model, "provider": "gemini-cli",
        "tokens.input": tok_in.to_string(),
        "tokens.output": tok_out.to_string(),
        "tokens.cache_read": tok_cached.to_string(),
        "tokens.reasoning": tok_thoughts.to_string(),
        "tokens.total": tok_total.to_string(),
        "tool.count": all_tools.len().to_string(),
        "tool.error_count": tool_error_count.to_string(),
        "turn.count": turn_count.to_string(),
        "thinking.block_count": thinking_count.to_string(),
        "stop_reason": "end_turn",
    });
    if interruption_count > 0 {
        root_attrs["has_interruption"] = json!("true");
        root_attrs["interruption.count"] = json!(interruption_count.to_string());
    }

    spans.push(make_span(&trace_id, root_sid, "", "gemini.session", t_start, t_end, 0, &root_attrs));
    spans.push(make_span(
        &trace_id, meta_sid, root_sid, "gemini.session.meta",
        t_start, t_start, 0,
        &json!({"session.id": session_id, "provider": "gemini-cli"}),
    ));

    // Tool spans
    for (i, t) in all_tools.iter().enumerate() {
        let span_id = pad16(i + 16);
        let is_err = t.status == "error" || t.status == "cancelled";
        let args = t.args.as_object();

        let mut attrs = json!({
            "tool.name": t.name,
            "tool.is_error": if is_err { "true" } else { "false" },
        });

        if let Some(args) = args {
            if let Some(fp) = args.get("file_path").and_then(|v| v.as_str())
                && !fp.is_empty()
            {
                attrs["tool.input.file_path"] = json!(fp);
            }
            if let Some(cmd) = args.get("command").and_then(|v| v.as_str())
                && !cmd.is_empty()
            {
                let truncated: String = cmd.chars().take(200).collect();
                attrs["tool.input.command"] = json!(truncated);
            }
            for key in &["query", "pattern"] {
                if let Some(val) = args.get(*key).and_then(|v| v.as_str())
                    && !val.is_empty()
                {
                    attrs["tool.input.pattern"] = json!(val);
                    break;
                }
            }
        }

        let end_ts = if t.timestamp.is_empty() { &t.msg_ts } else { &t.timestamp };
        spans.push(make_span(
            &trace_id, &span_id, root_sid,
            &format!("gemini.tool.{}", t.name),
            &t.msg_ts, end_ts,
            if is_err { 2 } else { 0 }, &attrs,
        ));
    }

    Ok(spans)
}

#[allow(clippy::too_many_arguments)]
fn make_span(trace_id: &str, span_id: &str, parent: &str, name: &str, start: &str, end: &str, status: u8, attrs: &Value) -> Value {
    json!({
        "trace_id": trace_id,
        "span_id": span_id,
        "parent_span_id": parent,
        "name": name,
        "start_ns": ts_to_ns(start),
        "end_ns": ts_to_ns(end),
        "status": status,
        "attributes": attrs,
    })
}
