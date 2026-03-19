# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

## [0.3.0] — 2026-03-19

### Added

- **Context breakdown** on Claude root span — 7 new attributes: `context.tool_output_chars`, `context.tool_output_tokens_est`, `context.user_prompt_chars`, `context.user_prompt_tokens_est`, `context.compact_summary_chars`, `context.compact_summary_tokens_est`, `context.compaction_pre_tokens`
- **Per-turn spans** (`claude.turn`) gated by `SHEPARD_DETAILED_TRACES=1` — per-turn token breakdown, tool count, cache stats (span offset 40016+)

## [0.2.0] — 2026-03-19

### Fixed

- Update `quinn-proto` to 0.11.14 (RUSTSEC-2026-0037, DoS — transitive via reqwest, QUIC not used)

### Added

- UTF-8 truncation test for codex parser

## [0.1.0] — 2026-03-06

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
- Git context extraction (repo name)
- Fire-and-forget OTLP emitters with `OTEL_HTTP_URL` env var support
- CI workflow (clippy + test + fmt + coverage) and release workflow (4 cross-compile targets)
- C4 architecture diagrams (system context, container, component, dynamic flow)

### Fixed

- Session finders for Codex/Gemini now search recursively (nested YYYY/MM/DD and project/chats dirs)
- PreToolUse checks only file paths, not commands (bash parity — avoids false positives)
- HTTP client uses 2s timeout to prevent blocking CLI on unreachable collector
- OTLP responses checked for HTTP error status (4xx/5xx logged instead of silently dropped)
- Parse errors in `parse_to_spans` logged to stderr instead of silently returning empty
