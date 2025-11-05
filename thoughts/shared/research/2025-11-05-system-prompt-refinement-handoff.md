---
date: 2025-11-05T02:55:00Z
author: Codex
topic: "Prompt Refinement Implementation Handoff"
scope: "System Prompt Refinement Surfaces"
---

# Prompt Refinement Implementation – Handoff Notes

## What’s Done
- **Structured instruction model**: `PromptInstruction`, `InstructionId`, scope/source/state enums, and `PromptStateSnapshot` now live under `crates/goose/src/agents/prompt_instruction.rs`. `PromptManager` tracks instructions in maps + ordered vectors, renders from `active_instruction_texts`, and can snapshot/restore (`crates/goose/src/agents/prompt_manager.rs`).
- **Persistence plumbing**: `PromptHistoryWriter` appends JSONL entries to `~/.config/goose/prompt-history/{hash}/{session}.jsonl`, while `PromptStateStore` saves/restores per-session JSON snapshots (`crates/goose/src/logging/prompt_history.rs`, `crates/goose/src/session/prompt_state.rs`). `Agent` persists/recovers state via `configure_prompt_persistence`, `persist_snapshot`, and `log_instruction_event`.
- **Config + CLI gating**: Introduced `PromptRefinementMode` (disabled | persist-only | enabled). `Config::prompt_refinement_mode()` reads `GOOSE_PROMPT_REFINEMENT`, and the CLI `--prompt-refinement` flag overrides per session. Session builder logs the active mode and only spins up persistence when not disabled (`crates/goose-cli/src/cli.rs`, `crates/goose-cli/src/session/builder.rs`).
- **Session bootstrap**: CLI session builder now calls `agent.configure_prompt_persistence(...)` before extension setup so resumed sessions hydrate from prior snapshots when enabled. Bench runner + other session entry points default the new config field.
- **Research log updates**: `thoughts/shared/research/2025-11-04-system-prompt-refinement-progress.md` captures code locations and implementation details for these changes.

## What’s Next
1. **Integrate lifecycle API everywhere**  
   - Replace direct `extend_system_prompt` calls in CLI, server routes, hint loader, recipes, final output tool, etc., with `extend_system_prompt_with_source` to preserve provenance metadata.  
   - Introduce scoped instructions (Persistent, Conversation, Tool) as appropriate; currently everything defaults to Session scope.
2. **Add user-facing controls**  
   - Implement `--prompt-log`, `--system-prompt-reset`, `--list-system-extras` CLI options described in the plan.  
   - Provide a `goose status` surface showing whether refinement is enabled and where logs live.
3. **Telemetry expansion**  
   - Extend `ProviderUsage` and session metrics to include instruction hashes/override hashes; emit Prometheus counters per source/action.  
   - Ensure compaction + retry flows propagate the additional metadata.
4. **Testing**  
   - Write unit tests for `PromptManager` lifecycle (duplicates, refresh, retire, snapshot).  
   - Integration tests for CLI flag behavior, persistence round-trips, and telemetry toggling (flag on/off).  
   - Update bench/recipe flows to verify prompt stacks behave deterministically.
5. **Validation runs (Phase 4 plan)**  
   - After wiring telemetry/tests, run `goose-self-test` in both baseline and refinement modes, collect token deltas, audit prompt-history artifacts, and document findings.

## Outstanding Considerations
- **Feature flag default**: currently `GOOSE_PROMPT_REFINEMENT` defaults to `disabled`. Decide rollout plan (e.g., `persist-only` in staging, `enabled` later) and document in `AGENTS.md`.
- **Security/privacy review**: storing sanitized previews may still leak sensitive snippets. Need guidance on redaction, retention, and whether to mirror logs inside repo for teams.
- **Server/UI parity**: CLI wiring exists, but server/desktop flows still bypass the lifecycle hooks. Need to expose configuration knobs for those surfaces and ensure multi-client sessions don’t fight over snapshots.
- **Cleaning snapshots/logs**: no GC yet. Add retention/cleanup similar to `logging::cleanup_old_logs`, possibly tied to session deletion.
- **Error UX**: persistence/logging failures currently warn but keep running; consider surfacing to users when `persist-only` or `enabled` mode silently downgrades.

## References
- Instruction model & manager: `crates/goose/src/agents/prompt_instruction.rs`, `crates/goose/src/agents/prompt_manager.rs`
- Agent persistence hooks: `crates/goose/src/agents/agent.rs`
- CLI session builder + flags: `crates/goose-cli/src/cli.rs`, `crates/goose-cli/src/session/builder.rs`
- Logging/state storage: `crates/goose/src/logging/prompt_history.rs`, `crates/goose/src/session/prompt_state.rs`
- Research log: `thoughts/shared/research/2025-11-04-system-prompt-refinement-progress.md`
