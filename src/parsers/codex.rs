use serde_json::{json, Value};
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::{BufRead, BufReader};

use super::common::{pad16, ts_to_ns};

/// Parse a Codex CLI JSONL session file and return spans as Vec<Value>.
pub fn parse_to_spans(file_path: &str) -> Vec<Value> {
    parse_inner(file_path).unwrap_or_default()
}

/// Parse a Codex CLI JSONL session file and emit span JSONL to stdout.
pub fn parse(file_path: &str) -> Result<(), Box<dyn Error>> {
    let spans = parse_inner(file_path)?;
    for span in &spans {
        println!("{}", serde_json::to_string(span).unwrap());
    }
    Ok(())
}

fn parse_inner(file_path: &str) -> Result<Vec<Value>, Box<dyn Error>> {
    let mut spans = Vec::new();
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);

    let entries: Vec<Value> = reader
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| serde_json::from_str(&l).ok())
        .collect();

    // Session metadata
    let meta = entries
        .iter()
        .find(|e| e["type"].as_str() == Some("session_meta"))
        .and_then(|e| e["payload"].as_object())
        .map(|o| Value::Object(o.clone()));

    let meta = match meta {
        Some(m) => m,
        None => return Ok(spans),
    };

    let session_id = meta["id"].as_str().unwrap_or("");
    if session_id.is_empty() {
        return Ok(spans);
    }

    let trace_id = session_id.replace('-', "");
    let root_sid = "0000000000000001";
    let meta_sid = "0000000000000002";

    // Model from first turn_context
    let model = entries
        .iter()
        .find_map(|e| {
            if e["type"].as_str() == Some("turn_context") {
                e["payload"]["model"].as_str()
            } else {
                None
            }
        })
        .unwrap_or("unknown");

    // Git info
    let git_branch = meta["git"]["branch"].as_str().unwrap_or("unknown");
    let git_repo = meta["git"]["repository_url"]
        .as_str()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .trim_end_matches(".git");

    // Timestamps
    let mut timestamps: Vec<&str> = entries
        .iter()
        .filter_map(|e| e["timestamp"].as_str())
        .collect();
    timestamps.sort();
    let t_start = timestamps.first().copied().unwrap_or("");
    let t_end = timestamps.last().copied().unwrap_or("");

    // Tokens from last token_count event
    let tok = entries
        .iter()
        .rev()
        .find_map(|e| {
            if e["type"].as_str() == Some("event_msg")
                && e["payload"]["type"].as_str() == Some("token_count")
                && !e["payload"]["info"].is_null()
            {
                Some(&e["payload"]["info"]["total_token_usage"])
            } else {
                None
            }
        });

    let tok_in = tok.and_then(|t| t["input_tokens"].as_i64()).unwrap_or(0);
    let tok_out = tok.and_then(|t| t["output_tokens"].as_i64()).unwrap_or(0);
    let tok_cache = tok.and_then(|t| t["cached_input_tokens"].as_i64()).unwrap_or(0);
    let tok_reasoning = tok.and_then(|t| t["reasoning_output_tokens"].as_i64()).unwrap_or(0);
    let tok_total = tok.and_then(|t| t["total_tokens"].as_i64()).unwrap_or(0);

    // Tool calls: join function_call → function_call_output by call_id
    let mut calls: HashMap<String, (String, String, String)> = HashMap::new(); // call_id → (name, ts, args)
    let mut call_order: Vec<String> = Vec::new();
    for e in &entries {
        if e["type"].as_str() == Some("response_item")
            && e["payload"]["type"].as_str() == Some("function_call")
        {
            let call_id = e["payload"]["call_id"].as_str().unwrap_or("").to_string();
            let name = e["payload"]["name"].as_str().unwrap_or("").to_string();
            let ts = e["timestamp"].as_str().unwrap_or("").to_string();
            let args = e["payload"]["arguments"].as_str().unwrap_or("{}").to_string();
            call_order.push(call_id.clone());
            calls.insert(call_id, (name, ts, args));
        }
    }

    let mut outputs: HashMap<String, String> = HashMap::new();
    for e in &entries {
        if e["type"].as_str() == Some("response_item")
            && e["payload"]["type"].as_str() == Some("function_call_output")
        {
            let call_id = e["payload"]["call_id"].as_str().unwrap_or("").to_string();
            let ts = e["timestamp"].as_str().unwrap_or("").to_string();
            outputs.insert(call_id, ts);
        }
    }

    struct ToolEntry {
        name: String,
        ts: String,
        end_ts: String,
        command: String,
        file_path: String,
    }

    let mut tools: Vec<ToolEntry> = Vec::new();
    for cid in &call_order {
        if let Some((name, ts, args_str)) = calls.get(cid) {
            let args: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
            let end_ts = outputs.get(cid).cloned().unwrap_or_else(|| ts.clone());
            let mut cmd = args["cmd"]
                .as_str()
                .or_else(|| args["command"].as_str())
                .unwrap_or("")
                .to_string();
            if cmd.len() > 200 {
                cmd.truncate(200);
            }
            tools.push(ToolEntry {
                name: name.clone(),
                ts: ts.clone(),
                end_ts,
                command: cmd,
                file_path: args["file_path"]
                    .as_str()
                    .or_else(|| args["path"].as_str())
                    .unwrap_or("")
                    .to_string(),
            });
        }
    }

    // Turns
    let turn_count = entries
        .iter()
        .filter(|e| {
            e["type"].as_str() == Some("event_msg")
                && e["payload"]["type"].as_str() == Some("task_started")
        })
        .count();

    // Compactions
    let compactions: Vec<&str> = entries
        .iter()
        .filter(|e| {
            e["type"].as_str() == Some("event_msg")
                && e["payload"]["type"].as_str() == Some("context_compacted")
        })
        .filter_map(|e| e["timestamp"].as_str())
        .collect();

    // Interruptions
    let interruption_count = entries
        .iter()
        .filter(|e| {
            e["type"].as_str() == Some("event_msg")
                && e["payload"]["type"].as_str() == Some("turn_aborted")
        })
        .count();

    // Stop reason
    let stop_reason = entries
        .iter()
        .rev()
        .find_map(|e| {
            if e["type"].as_str() == Some("event_msg") {
                match e["payload"]["type"].as_str() {
                    Some("task_complete") => Some("end_turn"),
                    Some("turn_aborted") => Some("interrupted"),
                    _ => None,
                }
            } else {
                None
            }
        })
        .unwrap_or("unknown");

    // ===== Emit spans =====

    let mut root_attrs = json!({
        "session.id": session_id, "model": model, "provider": "codex",
        "git.branch": git_branch, "git.repo": git_repo,
        "tokens.input": tok_in.to_string(),
        "tokens.output": tok_out.to_string(),
        "tokens.cache_read": tok_cache.to_string(),
        "tokens.reasoning": tok_reasoning.to_string(),
        "tokens.total": tok_total.to_string(),
        "tool.count": tools.len().to_string(),
        "turn.count": turn_count.to_string(),
        "compaction.count": compactions.len().to_string(),
        "stop_reason": stop_reason,
    });
    if interruption_count > 0 {
        root_attrs["has_interruption"] = json!("true");
        root_attrs["interruption.count"] = json!(interruption_count.to_string());
    }

    spans.push(make_span(&trace_id, root_sid, "", "codex.session", t_start, t_end, 0, &root_attrs));
    spans.push(make_span(
        &trace_id, meta_sid, root_sid, "codex.session.meta",
        t_start, t_start, 0,
        &json!({"session.id": session_id, "provider": "codex"}),
    ));

    // Tool spans
    for (i, t) in tools.iter().enumerate() {
        let span_id = pad16(i + 16);
        let mut attrs = json!({"tool.name": t.name});
        if !t.command.is_empty() {
            attrs["tool.input.command"] = json!(t.command);
        }
        if !t.file_path.is_empty() {
            attrs["tool.input.file_path"] = json!(t.file_path);
        }
        spans.push(make_span(&trace_id, &span_id, root_sid, &format!("codex.tool.{}", t.name), &t.ts, &t.end_ts, 0, &attrs));
    }

    // Compaction spans
    for (i, ts) in compactions.iter().enumerate() {
        let span_id = pad16(i + 30016);
        spans.push(make_span(&trace_id, &span_id, root_sid, "codex.compaction", ts, ts, 0, &json!({})));
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
