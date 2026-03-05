# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

## [0.1.0] — 2026-03-05

### Added

- **parse-session command** — parse Claude, Codex, and Gemini session files into JSONL spans
  - Claude: JSONL dedup by message.id, tool_use_id join, MCP + agent + compaction spans
  - Codex: function_call/output join by call_id, last token_count
  - Gemini: single JSON input, inline toolCalls extraction
- **emit-metric command** — POST OTLP Sum counter metric to OTel Collector
- **emit-traces command** — read JSONL spans from stdin, POST as OTLP traces
- **hook command** — full drop-in replacement for 9 bash hook scripts
  - Claude: PreToolUse (sensitive file guard), PostToolUse (metrics), Stop (session parse), SessionStart (compaction context)
  - Codex: Notify (agent-turn-complete filter, session parse)
  - Gemini: AfterTool, AfterModel, AfterAgent, SessionEnd
- Sensitive file detection (`.env`, credentials, `.pem`, `.key`, `id_rsa`, `.aws/`)
- Git context extraction (repo name + branch)
- Fire-and-forget OTLP emitters with `OTEL_HTTP_URL` env var support
- CI workflow (clippy + test + fmt) and release workflow (4 cross-compile targets)
- C4 architecture diagrams (system context, container, component, dynamic flow)
