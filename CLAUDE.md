# CLAUDE.md

## Overview

**shepard-hooks-rs** — Rust accelerator for shepard-obs-stack hooks.
Binary name: `shepard-hook`. Optional drop-in replacement for bash+jq hook logic.

If `shepard-hook` is on PATH, hooks use it. If absent, fall back to bash+jq (zero breakage).

## Commands

```bash
cargo build                    # debug build
cargo build --release          # optimized build (LTO + strip)
cargo test                     # run all tests
cargo clippy                   # lint (must be warning-free)
```

## CLI Interface

```
shepard-hook emit-metric <name> <value> <labels_json>
    → POST OTLP Sum metric to localhost:4318/v1/metrics

shepard-hook emit-traces <service_name>
    → stdin: JSONL spans → POST OTLP traces to localhost:4318/v1/traces

shepard-hook parse-session <provider> <file_path>
    → stdout: JSONL spans (same schema as bash parsers)
    → providers: claude, codex, gemini

shepard-hook hook <provider> <hook_name>
    → full hook replacement: stdin JSON → metrics + session parse
    → 9 hooks: claude/{pre-tool-use,post-tool-use,stop,session-start},
      codex/notify, gemini/{after-tool,after-model,after-agent,session-end}
```

## Architecture

```
src/
├── main.rs              ← CLI entry (clap)
├── cmd/
│   ├── emit_metric.rs   ← delegates to emit::metric()
│   ├── emit_traces.rs   ← delegates to emit::traces()
│   ├── parse_session.rs ← dispatch to provider parser
│   └── hook.rs          ← stdin → dispatch → HookOutput
├── hooks/
│   ├── mod.rs           ← HookHandler trait, dispatch(), detect_tool_error()
│   ├── context.rs       ← HookContext, session file finders
│   ├── claude.rs        ← PreToolUse, PostToolUse, Stop, SessionStart
│   ├── codex.rs         ← Notify
│   └── gemini.rs        ← AfterTool, AfterModel, AfterAgent, SessionEnd
├── parsers/
│   ├── common.rs        ← shared: pad16, ts_to_ns, subtract_ms
│   ├── claude.rs        ← port of session-parser.sh
│   ├── codex.rs         ← port of codex-session-parser.sh
│   └── gemini.rs        ← port of gemini-session-parser.sh
├── emit.rs              ← fire-and-forget OTLP POST (reads OTEL_HTTP_URL)
├── otlp.rs              ← OTLP JSON builders (metrics + traces)
├── git_context.rs       ← git repo/branch extraction
└── sensitive.rs         ← regex patterns for sensitive file detection
```

## Span Output Schema

All three parsers produce the same JSONL format (one JSON per line):
```json
{"trace_id":"...", "span_id":"...", "parent_span_id":"...", "name":"...",
 "start_ns":"...", "end_ns":"...", "status": 0, "attributes": {...}}
```

This is identical to the bash parser output, consumed by `emit-traces`.

## Key Crates

- `clap` — CLI parsing with derive
- `serde` / `serde_json` — JSON (zero-copy where possible)
- `reqwest` (blocking) — HTTP POST to OTel Collector
- `regex` — sensitive file patterns
- `chrono` — ISO 8601 → epoch nanos
