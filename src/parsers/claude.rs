use serde_json::{Value, json};
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::{BufRead, BufReader};

use super::common::{pad16, parts_to_ns, subtract_ms, ts_parts, ts_to_ns};

/// Parse a Claude Code JSONL session file and return spans as Vec<Value>.
/// Returns empty vec on any parse error or missing session ID.
pub fn parse_to_spans(file_path: &str) -> Vec<Value> {
    parse_inner(file_path).unwrap_or_else(|e| {
        eprintln!("shepard-hook: claude session parse failed: {e}");
        Vec::new()
    })
}

/// Parse a Claude Code JSONL session file and emit span JSONL to stdout.
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

    // Pre-filter: only assistant/user/progress/system entries
    let mut entries: Vec<Value> = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if (line.contains("\"type\":\"assistant\"")
            || line.contains("\"type\":\"user\"")
            || line.contains("\"type\":\"progress\"")
            || line.contains("\"type\":\"system\""))
            && let Ok(v) = serde_json::from_str::<Value>(&line)
        {
            entries.push(v);
        }
    }

    // Session ID
    let session_id = entries
        .iter()
        .find_map(|e| e["sessionId"].as_str())
        .unwrap_or_default();
    if session_id.is_empty() {
        return Ok(spans);
    }

    let trace_id = session_id.replace('-', "");
    let root_sid = "0000000000000001";
    let meta_sid = "0000000000000002";

    // Deduplicate assistant entries by message.id (keep last)
    let mut assistant_map: HashMap<String, Value> = HashMap::new();
    let mut assistant_order: Vec<String> = Vec::new();
    for e in &entries {
        if e["type"].as_str() == Some("assistant") {
            let msg_id = e["message"]["id"]
                .as_str()
                .or_else(|| e["uuid"].as_str())
                .unwrap_or("")
                .to_string();
            if !assistant_map.contains_key(&msg_id) {
                assistant_order.push(msg_id.clone());
            }
            assistant_map.insert(msg_id, e.clone());
        }
    }
    let assistants: Vec<Value> = assistant_order
        .iter()
        .filter_map(|id| assistant_map.get(id).cloned())
        .collect();

    // Rebuild all entries with deduped assistants, sorted by timestamp
    let mut all: Vec<Value> = entries
        .iter()
        .filter(|e| e["type"].as_str() != Some("assistant"))
        .cloned()
        .collect();
    all.extend(assistants.iter().cloned());
    all.sort_by(|a, b| {
        let ta = a["timestamp"].as_str().unwrap_or("");
        let tb = b["timestamp"].as_str().unwrap_or("");
        ta.cmp(tb)
    });

    // First/last timestamps
    let mut timestamps: Vec<&str> = all
        .iter()
        .filter_map(|e| {
            let t = e["type"].as_str()?;
            if (t == "user" || t == "assistant") && e["timestamp"].as_str().is_some() {
                e["timestamp"].as_str()
            } else {
                None
            }
        })
        .collect();
    timestamps.sort();
    let t_start = timestamps.first().copied().unwrap_or("");
    let t_end = timestamps.last().copied().unwrap_or("");

    // Model from first real assistant
    let model = assistants
        .iter()
        .find_map(|a| {
            let m = a["message"]["model"].as_str()?;
            if m != "<synthetic>" { Some(m) } else { None }
        })
        .unwrap_or("unknown");

    // Git context
    let git_branch = all
        .iter()
        .find_map(|e| e["gitBranch"].as_str())
        .unwrap_or("unknown");
    let git_repo = all.iter().find_map(|e| e["gitRepo"].as_str()).unwrap_or("");

    // Token aggregation
    let (mut tok_in, mut tok_out, mut tok_cache_read, mut tok_cache_create) =
        (0i64, 0i64, 0i64, 0i64);
    for a in &assistants {
        let u = &a["message"]["usage"];
        tok_in += u["input_tokens"].as_i64().unwrap_or(0);
        tok_out += u["output_tokens"].as_i64().unwrap_or(0);
        tok_cache_read += u["cache_read_input_tokens"].as_i64().unwrap_or(0);
        tok_cache_create += u["cache_creation_input_tokens"].as_i64().unwrap_or(0);
    }
    let tok_total = tok_in + tok_out + tok_cache_read + tok_cache_create;

    // Stop reason
    let stop_reason = assistants
        .iter()
        .rev()
        .find_map(|a| a["message"]["stop_reason"].as_str())
        .unwrap_or("unknown");

    // Thinking block count
    let thinking_count: usize = assistants
        .iter()
        .map(|a| {
            a["message"]["content"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter(|c| c["type"].as_str() == Some("thinking"))
                        .count()
                })
                .unwrap_or(0)
        })
        .sum();

    // User interruptions
    let interruption_count: usize = all
        .iter()
        .filter(|e| {
            e["type"].as_str() == Some("user") && {
                let content = &e["message"]["content"];
                let text = if content.is_string() {
                    content.as_str().unwrap_or("")
                } else {
                    ""
                };
                text.contains("Request interrupted by user")
            }
        })
        .count();

    // Compaction events
    let compactions: Vec<(&str, &str, i64)> = all
        .iter()
        .filter(|e| {
            e["type"].as_str() == Some("system")
                && e["subtype"].as_str() == Some("compact_boundary")
        })
        .map(|e| {
            let ts = e["timestamp"].as_str().unwrap_or("");
            let trigger = e["compactMetadata"]["trigger"].as_str().unwrap_or("auto");
            let pre_tokens = e["compactMetadata"]["preTokens"].as_i64().unwrap_or(0);
            (ts, trigger, pre_tokens)
        })
        .collect();

    // Tool result lookup: tool_use_id → (timestamp, is_error)
    let mut results: HashMap<String, (String, bool)> = HashMap::new();
    for e in &all {
        if e["type"].as_str() != Some("user") {
            continue;
        }
        if let Some(content) = e["message"]["content"].as_array() {
            for c in content {
                if c["type"].as_str() == Some("tool_result") {
                    let id = c["tool_use_id"].as_str().unwrap_or("").to_string();
                    let ts = e["timestamp"].as_str().unwrap_or("").to_string();
                    let err = c["is_error"].as_bool().unwrap_or(false);
                    results.insert(id, (ts, err));
                }
            }
        }
    }

    // Tool use entries
    struct ToolEntry {
        id: String,
        name: String,
        ts: String,
        file_path: String,
        command: String,
        pattern: String,
        tokens_out: i64,
    }
    let mut tools: Vec<ToolEntry> = Vec::new();
    for a in &assistants {
        if let Some(content) = a["message"]["content"].as_array() {
            for c in content {
                if c["type"].as_str() == Some("tool_use") {
                    let mut cmd = c["input"]["command"].as_str().unwrap_or("").to_string();
                    if cmd.len() > 200 {
                        cmd.truncate(200);
                    }
                    tools.push(ToolEntry {
                        id: c["id"].as_str().unwrap_or("").to_string(),
                        name: c["name"].as_str().unwrap_or("").to_string(),
                        ts: a["timestamp"].as_str().unwrap_or("").to_string(),
                        file_path: c["input"]["file_path"]
                            .as_str()
                            .or_else(|| c["input"]["notebook_path"].as_str())
                            .unwrap_or("")
                            .to_string(),
                        command: cmd,
                        pattern: c["input"]["pattern"]
                            .as_str()
                            .or_else(|| c["input"]["query"].as_str())
                            .unwrap_or("")
                            .to_string(),
                        tokens_out: a["message"]["usage"]["output_tokens"].as_i64().unwrap_or(0),
                    });
                }
            }
        }
    }

    let tool_error_count: usize = tools
        .iter()
        .filter(|t| results.get(&t.id).is_some_and(|(_, err)| *err))
        .count();

    // Turn count
    let turn_count: usize = all
        .iter()
        .filter(|e| {
            if e["type"].as_str() != Some("user") {
                return false;
            }
            let content = &e["message"]["content"];
            if content.is_string() {
                true
            } else if let Some(arr) = content.as_array() {
                !arr.iter()
                    .any(|c| c["type"].as_str() == Some("tool_result"))
            } else {
                false
            }
        })
        .count();

    // MCP completed entries
    struct McpEntry {
        ts: String,
        server: String,
        tool: String,
        elapsed_ms: i64,
    }
    let mcps: Vec<McpEntry> = all
        .iter()
        .filter(|e| {
            e["type"].as_str() == Some("progress")
                && e["data"]["type"].as_str() == Some("mcp_progress")
                && e["data"]["status"].as_str() == Some("completed")
        })
        .map(|e| McpEntry {
            ts: e["timestamp"].as_str().unwrap_or("").to_string(),
            server: e["data"]["serverName"]
                .as_str()
                .unwrap_or("unknown")
                .to_string(),
            tool: e["data"]["toolName"]
                .as_str()
                .unwrap_or("unknown")
                .to_string(),
            elapsed_ms: e["data"]["elapsedTimeMs"].as_i64().unwrap_or(0),
        })
        .collect();

    // Agent progress grouped by agentId
    struct AgentEntry {
        ts: String,
        prompt: String,
    }
    let mut agent_groups: HashMap<String, Vec<AgentEntry>> = HashMap::new();
    for e in &all {
        if e["type"].as_str() == Some("progress")
            && e["data"]["type"].as_str() == Some("agent_progress")
            && e["data"]["message"]["type"].as_str() == Some("user")
        {
            let aid = e["data"]["agentId"].as_str().unwrap_or("").to_string();
            let mut prompt = e["data"]["prompt"].as_str().unwrap_or("").to_string();
            if prompt.len() > 80 {
                prompt.truncate(80);
            }
            agent_groups
                .entry(aid.clone())
                .or_default()
                .push(AgentEntry {
                    ts: e["timestamp"].as_str().unwrap_or("").to_string(),
                    prompt,
                });
        }
    }

    // ===== Emit spans =====

    // 1. Root session span
    let mut root_attrs = json!({
        "session.id": session_id, "model": model, "provider": "claude-code",
        "git.branch": git_branch, "git.repo": git_repo,
        "tokens.input": tok_in.to_string(),
        "tokens.output": tok_out.to_string(),
        "tokens.cache_read": tok_cache_read.to_string(),
        "tokens.cache_create": tok_cache_create.to_string(),
        "tokens.total": tok_total.to_string(),
        "tool.count": tools.len().to_string(),
        "tool.error_count": tool_error_count.to_string(),
        "turn.count": turn_count.to_string(),
        "compaction.count": compactions.len().to_string(),
        "thinking.block_count": thinking_count.to_string(),
        "stop_reason": stop_reason,
    });
    if interruption_count > 0 {
        root_attrs["has_interruption"] = json!("true");
        root_attrs["interruption.count"] = json!(interruption_count.to_string());
    }

    spans.push(make_span(
        &trace_id,
        root_sid,
        "",
        "claude.session",
        t_start,
        t_end,
        0,
        &root_attrs,
    ));

    // 1b. Session meta marker
    spans.push(make_span(
        &trace_id,
        meta_sid,
        root_sid,
        "claude.session.meta",
        t_start,
        t_start,
        0,
        &json!({"session.id": session_id, "provider": "claude-code"}),
    ));

    // 2. Tool call spans (offset: 16)
    for (i, t) in tools.iter().enumerate() {
        let span_id = pad16(i + 16);
        let (end_ts, is_err) = results
            .get(&t.id)
            .map(|(ts, err)| (ts.as_str(), *err))
            .unwrap_or((&t.ts, false));

        let mut attrs = json!({
            "tool.name": t.name,
            "tokens.output": t.tokens_out.to_string(),
            "tool.is_error": if is_err { "true" } else { "false" },
        });
        if !t.file_path.is_empty() {
            attrs["tool.input.file_path"] = json!(t.file_path);
        }
        if !t.command.is_empty() {
            attrs["tool.input.command"] = json!(t.command);
        }
        if !t.pattern.is_empty() {
            attrs["tool.input.pattern"] = json!(t.pattern);
        }

        spans.push(make_span(
            &trace_id,
            &span_id,
            root_sid,
            &format!("claude.tool.{}", t.name),
            &t.ts,
            end_ts,
            if is_err { 2 } else { 0 },
            &attrs,
        ));
    }

    // 3. MCP call spans (offset: 10016)
    for (i, m) in mcps.iter().enumerate() {
        let span_id = pad16(i + 10016);
        let end_parts = ts_parts(&m.ts);
        let start_parts = subtract_ms(&end_parts, m.elapsed_ms);
        let start_ns = parts_to_ns(&start_parts);
        let end_ns = ts_to_ns(&m.ts);

        spans.push(make_span_raw(
            &trace_id, &span_id, root_sid,
            &format!("claude.mcp.{}.{}", m.server, m.tool),
            &start_ns, &end_ns, 0,
            &json!({"mcp.server": m.server, "mcp.tool": m.tool, "mcp.duration_ms": m.elapsed_ms.to_string()}),
        ));
    }

    // 4. Sub-agent spans (offset: 20016)
    let mut agent_keys: Vec<&String> = agent_groups.keys().collect();
    agent_keys.sort();
    for (i, aid) in agent_keys.iter().enumerate() {
        let group = &agent_groups[*aid];
        let span_id = pad16(i + 20016);
        let mut tss: Vec<&str> = group.iter().map(|a| a.ts.as_str()).collect();
        tss.sort();
        let prompt = group.first().map(|a| a.prompt.as_str()).unwrap_or("");

        spans.push(make_span(
            &trace_id,
            &span_id,
            root_sid,
            &format!("claude.agent.{}", aid),
            tss.first().unwrap_or(&""),
            tss.last().unwrap_or(&""),
            0,
            &json!({"agent.id": *aid, "agent.prompt": prompt}),
        ));
    }

    // 5. Compaction spans (offset: 30016)
    for (i, (ts, trigger, pre_tokens)) in compactions.iter().enumerate() {
        let span_id = pad16(i + 30016);
        spans.push(make_span(
            &trace_id, &span_id, root_sid, "claude.compaction",
            ts, ts, 0,
            &json!({"compaction.trigger": trigger, "compaction.pre_tokens": pre_tokens.to_string()}),
        ));
    }

    Ok(spans)
}

#[allow(clippy::too_many_arguments)]
fn make_span(
    trace_id: &str,
    span_id: &str,
    parent: &str,
    name: &str,
    start: &str,
    end: &str,
    status: u8,
    attrs: &Value,
) -> Value {
    make_span_raw(
        trace_id,
        span_id,
        parent,
        name,
        &ts_to_ns(start),
        &ts_to_ns(end),
        status,
        attrs,
    )
}

#[allow(clippy::too_many_arguments)]
fn make_span_raw(
    trace_id: &str,
    span_id: &str,
    parent: &str,
    name: &str,
    start_ns: &str,
    end_ns: &str,
    status: u8,
    attrs: &Value,
) -> Value {
    json!({
        "trace_id": trace_id,
        "span_id": span_id,
        "parent_span_id": parent,
        "name": name,
        "start_ns": start_ns,
        "end_ns": end_ns,
        "status": status,
        "attributes": attrs,
    })
}
