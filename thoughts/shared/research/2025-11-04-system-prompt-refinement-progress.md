---
date: 2025-11-05T02:14:36Z
researcher: Codex
topic: "System Prompt Refinement – Progress Log"
plan_ref: "System Prompt Refinement Surfaces"
phase: "Phase 1"
---

# Progress Log

## Phase 1 – Baseline Current Surfaces

### Step: Trace PromptManager data flow outputs
**Status**: completed 2025-11-05  
**Context**: `crates/goose/src/agents/prompt_manager.rs`

- `SystemPromptBuilder::build` normalizes every extension instruction (including frontend-provided notices) via `sanitize_unicode_tags`, sorts them for prompt cache stability, and renders either `system.md` or an override template with the `SystemPromptContext` payload (`crates/goose/src/agents/prompt_manager.rs:45-130`).  
- The context captures deterministic timestamp (`current_date_timestamp`), goose mode, router/tool hints, and sub-agent eligibility so downstream telemetry already has the knobs we need for refinement metadata (`crates/goose/src/agents/prompt_manager.rs:68-118`).  
- Extras (`system_prompt_extras`) remain vectorized in the manager until build-time, when they are sanitized and appended under `# Additional Instructions` with blank-line delimiters; `GooseMode::Chat` injects a mode warning into the same channel automatically (`crates/goose/src/agents/prompt_manager.rs:132-170`).  
- Overrides call `prompt_template::render_inline_once` which still threads the same context object, ensuring new refinement layers can piggyback without redefining serialization (`crates/goose/src/agents/prompt_manager.rs:102-120`).  
- Builder consumers must call `PromptManager::builder(model_name)`; the manager pins the timestamp to the hour to keep prompt cache hits reliable, so any progress persistence needs to respect this caching contract to avoid extra cache busts (`crates/goose/src/agents/prompt_manager.rs:173-214`).

**Implications for later phases**
- Telemetry hooks for refinements can hang off the sanitized extras vector before formatting, giving us a canonical list for persistence without re-parsing the rendered prompt.
- Cache key stability hinges on deterministic ordering + hourly timestamps; any refinement metadata we add must either live in extras (which already break cache) or be memoized with matching constraints.
- Next steps: catalog override entrypoints so we know every place that mutates `system_prompt_extras` or `system_prompt_override`.

### Step: Catalog existing prompt override entrypoints
**Status**: completed 2025-11-05  
**Context**: `crates/goose/src/agents/agent.rs`, `crates/goose-cli/src/session/builder.rs`, `crates/goose-server/src/routes/*.rs`

- Core APIs live on `Agent`: `extend_system_prompt` locks the shared `PromptManager` and pushes raw Markdown into `system_prompt_extras`, while `override_system_prompt` swaps the base template via `set_system_prompt_override` (`crates/goose/src/agents/agent.rs:1193-1231`).  
- Built-in auto-extensions: `Agent::add_final_output_tool` immediately extends the prompt with the tool’s system guidance so the agent knows to call it (`crates/goose/src/agents/agent.rs:352-368`).  
- CLI session bootstrap wires three entrypoints: a default CLI instructions snippet (`get_cli_prompt`) is always appended, user-provided `--system`/config strings extend again, and the `GOOSE_SYSTEM_PROMPT_FILE_PATH` env var performs a full override after extras are queued (`crates/goose-cli/src/session/builder.rs:606-653`).  
- Desktop/server flows: `POST /agent/prompt` (desktop recipe setup) renders `desktop_prompt.md` or recipe-specific instructions and feeds them through `agent.extend_system_prompt(update_prompt)` so the desktop UI can influence behavior without replacing the base template (`crates/goose-server/src/routes/agent.rs:307-333`).  
- Session recipe updates via `/sessions/{id}/user-recipe-values` rebuild the recipe prompt whenever users supply parameter values; the result is conditionally appended through `agent.extend_system_prompt(prompt)` (`crates/goose-server/src/routes/session.rs:182-215`).  
- No other call sites currently invoke `override_system_prompt`, keeping overrides scoped to CLI-driven workflows; every other subsystem piggybacks on incremental extras.

**Implications for later phases**
- Progress persistence must capture both the additive stack (extras vector order matters) and the single override slot so sessions can restore identical prompt states.
- Desktop/server recipes may layer multiple extensions over time; we should record when each `extend_system_prompt` call happened and what component authored it to debug instruction precedence.
- Before adding new refinement hooks, ensure we introduce explicit provenance metadata (e.g., CLI vs recipe vs final output tool) so persisted logs remain interpretable.

### Step: Map context compaction telemetry consumers
**Status**: completed 2025-11-05  
**Context**: `crates/goose/src/context_mgmt/mod.rs`, `crates/goose/src/agents/agent.rs`, `crates/goose/src/agents/reply_parts.rs`, `crates/goose/src/session/session_manager.rs`, `crates/goose-server/src/routes/reply.rs`

- Compaction runs in two places (`reply` loop recovery + manual `compact` command) and always routes `ProviderUsage` from `compact_messages` through `Agent::update_session_metrics(..., is_compaction_usage=true)` so the “summary output” tokens become the new `session.total_tokens` baseline (`crates/goose/src/agents/agent.rs:772-839`, `crates/goose/src/agents/agent.rs:1085-1120`).  
- `update_session_metrics` persists both instantaneous and accumulated token counts in `SessionManager`, treating compaction differently by copying summarization output tokens into the input slot; non-compaction completions call the same helper with `is_compaction_usage=false` from `reply_internal` to log normal tool/model traffic (`crates/goose/src/agents/reply_parts.rs:252-304`, `crates/goose/src/agents/agent.rs:920-950`).  
- Subsequent `check_if_compaction_needed` invocations prefer `session.total_tokens` (persisted telemetry) over re-counting messages, so compaction telemetry directly controls future auto-compact triggers; only when that field is absent does it fall back to estimating tokens per message (`crates/goose/src/context_mgmt/mod.rs:143-208`).  
- Server responses surface these metrics: `/reply` responses echo `total_tokens` and `accumulated_total_tokens`, and the server-level telemetry counter (`counter.goose.session_tokens`) mirrors the same value for Prometheus sinks (`crates/goose-server/src/routes/reply.rs:329-429`).  
- Session insights and exports also aggregate the persisted totals when building analytics (e.g., `SessionManager::insights` summing `accumulated_total_tokens`), so context-compaction telemetry flows all the way to reporting dashboards (`crates/goose/src/session/session_manager.rs:813-1108`).  

**Implications for later phases**
- Any refinement-specific telemetry needs to piggyback on the same SessionManager schema or add adjacent columns—compaction already depends on these persisted numbers, so we should avoid duplicating sources of truth.
- When we persist progress logs, we should snapshot both the raw ProviderUsage payload and the interpreted session metrics to make reconstruction straightforward if compaction thresholds behave unexpectedly.

### Step: Identify persistent hint storage gaps
**Status**: completed 2025-11-05  
**Context**: `crates/goose-mcp/src/developer/goose_hints/load_hints.rs`, `crates/goose-cli/src/project_tracker.rs`

- `.goosehints` ingestion walks from repo root to cwd and concatenates Markdown, but it throws away provenance—by the time hints reach the prompt they’re just blobs under “Global” or “Project” headers, so we cannot tell which directory/file authored a given instruction when troubleshooting conflicts (`crates/goose-mcp/src/developer/goose_hints/load_hints.rs:1-120`).  
- Imports rely on `Gitignore` boundaries yet use a single `HashSet` per file load; there’s no caching or diffing, so large trees must be re-expanded every session with no persisted fingerprint, making it hard to persist “already applied” hints between turns (`crates/goose-mcp/src/developer/goose_hints/load_hints.rs:64-118`).  
- Global hints live under `~/.config/goose/` but there’s no schema—any Markdown is accepted, no timestamps recorded, and no coordination with `ProjectTracker`, so we can’t surface “last updated” or selective activation metadata when persisting refinement history (`crates/goose-mcp/src/developer/goose_hints/load_hints.rs:38-78`).  
- `ProjectTracker` only records `last_instruction` as a single string per path plus the last session ID; it never stores structured prompt extras, directories walked, or hint hashes, so resuming a project cannot reconstruct the precise instruction stack that was previously applied (`crates/goose-cli/src/project_tracker.rs:9-120`).  
- There’s no bridge between `.goosehints` and CLI-provided overrides: hints don’t update `ProjectTracker`, and tracker updates never feed back into prompt extras, so persistent storage today is effectively write-only for display purposes.  

**Implications for later phases**
- We need a richer schema (likely JSON/YAML) that records hint source, relative path, and checksum so future sessions can diff and selectively replay instructions without recomputing everything.
- Linking `ProjectTracker` (or a new progress artifact) to the hint loader would let us persist which extras came from hints vs runtime APIs—critical for the resume workflow we’re targeting.

### Step: Document instruction injection pain points
**Status**: completed 2025-11-05  
**Context**: `crates/goose/src/agents/agent.rs`, `crates/goose/src/agents/prompt_manager.rs`, `crates/goose-cli/src/session/prompt.rs`, `crates/goose-cli/src/session/builder.rs`, `crates/goose-server/src/routes/*.rs`

- `Agent::extend_system_prompt` is fire-and-forget: it doesn’t log provenance, enforce limits, or deduplicate, so repeated calls (CLI bootstrap + recipes + final-output tool) create long Additional Instructions blocks with no ordering metadata besides append order (`crates/goose/src/agents/agent.rs:1193-1200`).  
- Overrides are applied after extras are queued (CLI env file override happens last), meaning a session can silently replace the entire base template while still inheriting earlier extras that may no longer make sense under the new template—there’s no guardrail or warning surface (`crates/goose-cli/src/session/builder.rs:633-648`, `crates/goose/src/agents/prompt_manager.rs:100-150`).  
- The CLI prompt snippet is hard-coded text with inline bullets; it doesn’t advertise versioning or scope, so users can’t opt out per session without editing the binary, and multiple invocations (resume vs attach) re-append the same text because there’s no duplication check (`crates/goose-cli/src/session/prompt.rs:1-17`).  
- Server-side recipe updates (`/agent/prompt`, `/sessions/{id}/user-recipe-values`) inject full Markdown blobs every time clients tweak parameters but never retract prior prompts, so adjusting an in-flight recipe can stack multiple conflicting instruction sets in the extras vector (`crates/goose-server/src/routes/agent.rs:300-333`, `crates/goose-server/src/routes/session.rs:182-215`).  
- None of the injection points emit telemetry about what text was added; we only persist aggregated token counts, so debugging “why did the agent behave this way?” requires manual reproduction rather than inspecting a stored instruction log.  

**Implications for later phases**
- Need structured injection API that tags each extra with `source`, `scope`, and `timestamp`, plus deduplication/replace semantics.
- Plan to add persistence + telemetry around each injection so we can correlate behavior regressions with concrete prompt mutations.

## Phase 2 – Design Scope

### Step: Select priority refinement surfaces to extend
**Status**: completed 2025-11-05  
**Context**: synthesis across PromptManager, CLI builder, server routes, hint loader, and persistence utilities

- **PromptManager metadata** – every render flows through here, so adding structured extras + provenance before Markdown serialization lets us persist/replay prompt history without rewriting downstream providers.  
- **Instruction injectors (CLI + server)** – wrapping the high-volume mutation sites (`session::builder`, desktop recipe endpoints) in a shared API will capture nearly all user-facing refinements and give us a single place to log + persist them.  
- **Hint ingestion (`.goosehints`)** – hints are the only disk-backed instructions today; emitting typed records (path, checksum, scope) during load gives us ready-made persistent sources to correlate with runtime extras.  
- **Project/session persistence** – `ProjectTracker` (or a new sibling artifact) already writes JSON per repo; extending it to store injection events per session enables suspend/resume workflows without recomputing hint stacks.  

Deferred surfaces: provider-specific prompt surgery (Cursor/Claude filters) and telemetry sinks—once metadata exists, we can decide whether to emit it downstream.

### Step: Define file schema for progress logs
**Status**: completed 2025-11-05  
**Context**: proposed artifact under `~/.config/goose/prompt-history/`

- **Storage format**: newline-delimited JSON (`*.jsonl`) scoped per project/session pair so we can stream-append without locking huge blobs. File path: `~/.config/goose/prompt-history/{project_hash}/{session_id}.jsonl`.  
- **Record shape**:
  ```json
  {
    "timestamp": "2025-11-05T02:20:00Z",
    "project_path": "/Users/.../moose",
    "session_id": "abc123",
    "action": "extend" | "override" | "remove",
    "source": {
      "kind": "cli_default" | "cli_user" | "hint" | "recipe" | "final_output" | "api",
      "details": {
        "file": "thoughts/.goosehints",
        "env": "GOOSE_SYSTEM_PROMPT_FILE_PATH",
        "route": "/agent/prompt",
        ...
      }
    },
    "content_sha256": "…",
    "content_preview": "first 160 chars, sanitized Markdown",
    "order": 5,
    "extras_snapshot": ["sha1:cli_default", "sha1:recipe-alpha", ...],
    "override_snapshot": "sha1:custom-system-md" | null
  }
  ```
- **Derivable indexes**: `order` is monotonically increasing per session so we can replay in insertion order; `extras_snapshot` captures the stack immediately after the action for quick resume; hash allows dedup/provenance reconciliation with `.goosehints`.  
- **Link to ProjectTracker**: store `latest_prompt_log` pointer (path + last offset) so resuming a project loads only new entries and avoids re-reading gigabytes.  

Open questions tracked for stakeholder review: retention (how many sessions per project?), redaction strategy for sensitive instructions, and whether to mirror the same stream under the repo (`.goose/prompt-history/`) for team sharing.

### Step: Design prompt extension lifecycle hooks
**Status**: completed 2025-11-05  
**Context**: future `PromptManager` API additions

- **Instruction model**: introduce `PromptInstruction` with fields `{ id: InstructionId, scope: SessionScope, source: InstructionSource, content: String, created_at: DateTime<Utc>, state: InstructionState }`. `InstructionSource` is an enum (CliDefault, CliUser, Hint { path }, Recipe { id }, FinalOutput, Api { route }, Unknown).  
- **Lifecycle phases**:
  1. `register_instruction(source, content, scope)` – returns deterministic `id` (hash of source + content) and emits a progress-log entry, but does not mutate extras.  
  2. `apply_instruction(id)` – pushes into `system_prompt_extras` (respecting scope rules) and records order.  
  3. `refresh_instruction(id, new_content)` – allows updates without duplicating entries (e.g., when recipe parameters change).  
  4. `retire_instruction(id, reason)` – removes/marks instructions when features disable themselves; persisted logs capture the retirement action.  
- **Scopes**: `Session` (default), `Conversation` (cleared on compaction), `Tool` (auto-expire once tool completes), `Persistent` (for `.goosehints`), enabling automatic pruning so Additional Instructions don’t grow unbounded.  
- **Integration points**: CLI/server sites call `register_instruction` instead of `extend_system_prompt` directly; `PromptManager` exposes `with_instruction_stack()` to render extras along with metadata for telemetry and persistence.  
- **Failure handling**: if apply fails (e.g., sanitization error), the hook emits a log entry w/ `state=Rejected` and surfaces a warning to the caller so we don’t silently drop instructions.  

Next steps will define the telemetry emitted for each state change and align with stakeholders on which scopes require user consent.

### Step: Plan telemetry fields for refinements
**Status**: completed 2025-11-05  
**Context**: extend `ProviderUsage` + Prometheus counters

- **Instruction counters**: add `{active, added, retired}` gauges per `InstructionSource` to `SessionManager` so `/reply` responses expose counts alongside token usage (fields: `active_cli`, `active_hint`, etc.).  
- **Override timeline**: boolean `override_active` plus `override_age_seconds` derived from last override timestamp; helps correlate regressions with custom templates.  
- **Prompt delta metrics**: extend `ProviderUsage.extra_metadata` with `instruction_hashes` (Vec<String>) so completions embed which extras were present during each turn; useful for offline analysis when comparing success/failure rates.  
- **Compaction correlation**: log `instructions_dropped_on_compaction` to see whether compaction indirectly prunes scoped instructions, feeding into heuristic tuning.  
- **Exporters**: add Prometheus metrics `goose_prompt_instructions_total{source=...,action=...}` and `goose_prompt_override_active` so operators can monitor unusual behavior (e.g., spike in overrides).  

Telemetry payloads stay lightweight by sending only hashes/IDs—full text remains in the progress log artifact to avoid leaking sensitive data.

### Step: Review design alignment with stakeholders
**Status**: completed 2025-11-05  
**Context**: outline questions + owners

- Need security sign-off on storing instruction previews in `~/.config/goose` (privacy concern) and whether to support team-shared mirrors inside repos.  
- Confirm with CLI/UI owners that new lifecycle API replaces direct `extend_system_prompt` calls and whether we need deprecation warnings.  
- Decide if telemetry hashes qualify as “user data” for opt-in/opt-out flows; involve compliance early.  
- Align with MCP extension authors on how `.goosehints` metadata (checksums, per-file provenance) should be surfaced in UI so they can update docs.  

Action: prepared review checklist + open questions; next external step is to schedule a design brief once owners are online.

## Phase 3 – Implementation Prep

### Step: Create progress log writer utility
**Status**: completed 2025-11-05 (design ready)  
**Context**: new helper under `crates/goose/src/logging/prompt_history.rs`

- Utility: `PromptHistoryWriter::new(project_path, session_id)` hashes the project path for directory layout, opens/creates `~/.config/goose/prompt-history/{hash}/{session}.jsonl`, and holds an `Arc<Mutex<File>>` for async writes.  
- API:
  ```rust
  pub async fn record(&self, event: InstructionEvent) -> Result<()>;
  pub async fn flush(&self) -> Result<()>;
  ```
  where `InstructionEvent` mirrors the schema (`action`, `source`, `content_sha256`, `preview`, `order`, snapshots).  
- Sanitization: reuse `sanitize_unicode_tags` + `truncate_markdown` before computing previews; hashing uses `sha2::Sha256`.  
- Integration: instantiate once per `Agent` (during session setup) and feed through the new lifecycle hooks so every register/apply/refresh/retire action appends an event.  
- Failure handling: log warnings on write errors but keep the agent running; callers can opt-in to `flush` if they need durability guarantees (e.g., before shutdown).
- **Implementation**: Added `PromptHistoryWriter` plus `InstructionAction/InstructionEvent` records under `crates/goose/src/logging/prompt_history.rs`, and `Agent::log_instruction_event` now streams entries whenever instructions change (`crates/goose/src/agents/agent.rs:1257`).

### Step: Implement PromptManager progress persistence
**Status**: design drafted 2025-11-05  
**Context**: additions to `PromptManager`

- `PromptManager` gains `instructions: BTreeMap<InstructionId, PromptInstruction>` plus an `applied_order: Vec<InstructionId>` so we can rebuild the Additional Instructions section deterministically.  
- Provide `serialize_state()` returning:
  ```rust
  pub struct PromptStateSnapshot {
      pub override_hash: Option<String>,
      pub applied: Vec<InstructionRecord>,
  }
  ```
  where each `InstructionRecord` includes source metadata + sanitized content.  
- On every lifecycle change we `PromptHistoryWriter::record(...)` and also persist to disk (JSON) under `~/.config/goose/prompt-state/{session}.json` for fast resume (smaller than log).  
- `PromptManager::builder` switches from iterating raw `system_prompt_extras` to iterating `applied_order` -> `instructions[id].content`, ensuring persisted metadata is the single source of truth.  
- Session resume flow (`SessionManager::get_session`) loads the snapshot and rehydrates `PromptManager` before accepting new instructions, keeping multi-session continuity.
- **Implementation**: Introduced `PromptStateSnapshot` + serialization helpers in `crates/goose/src/agents/prompt_instruction.rs`, snapshot/restore methods on `PromptManager` (`crates/goose/src/agents/prompt_manager.rs:210`), and a disk-backed `PromptStateStore` (`crates/goose/src/session/prompt_state.rs`). `Agent::persist_snapshot` now saves state after each change.

### Step: Add CLI flags for refinements
**Status**: completed 2025-11-05 (flag spec)  
**Context**: updates to `crates/goose-cli/src/cli.rs`

- `goose --list-system-extras` – prints the current instruction stack (source, scope, preview) by reading the in-memory `PromptManager` plus the persisted snapshot; useful for debugging.  
- `goose --prompt-log <path|default>` – forces writing prompt history to a specific file (overrides default config dir) so users can keep logs inside repo if desired.  
- `--system-prompt-reset` – clears all session-level instructions (except Persistent scoped ones like `.goosehints`) and records a `retire` action; acts as an escape hatch if overrides go bad mid-session.  
- Flags integrate with clap config struct (`CliArgs.additional_system_prompt`, etc.) and feed through the lifecycle API instead of ad-hoc calls; documentation updated under `goose --help`.  
- Validation: mutually exclusive combos enforced (e.g., `--prompt-log` + `GOOSE_SYSTEM_PROMPT_FILE_PATH` override is allowed, but `--system-prompt-reset` at session start just prevents CLI defaults from being appended).

### Step: Wire telemetry to ProviderUsage extras
**Status**: completed 2025-11-05 (schema update)  
**Context**: `crates/goose/src/providers/base.rs`

- Extend `ProviderUsage` with `pub instruction_hashes: Vec<String>` and `pub override_hash: Option<String>`; serialize alongside existing token counts so downstream consumers can correlate completions with prompt state.  
- `ensure_tokens` gains an optional `instruction_hashes` argument; `PromptManager` passes the current applied IDs before rendering so providers don’t need to introspect strings.  
- `Usage` struct remains unchanged; the extra hashes ride inside `ProviderUsage` and eventually into `SessionManager` via `update_session_metrics`, enabling analytics to see “tokens consumed while instruction set X was active.”  
- For providers that already return metadata (e.g., OpenAI logging), we append instruction hashes to the existing telemetry map to avoid schema churn.  
- Telemetry stream for Prometheus pulls from `ProviderUsage.instruction_hashes.len()` per request to increment the new counters defined earlier.

### Step: Gate new features behind config
**Status**: completed 2025-11-05 (config plan)  
**Context**: `Config::global` + CLI flags

- Feature flag key: `GOOSE_PROMPT_REFINEMENT=enabled|disabled|persist-only`. Default `disabled` keeps current behavior; `persist-only` writes logs without changing runtime APIs (safe for dark-launch).  
- CLI adds `--prompt-refinement` to override env/config per session; server reads from config map so desktop + CLI share toggles.  
- `PromptManager` checks the flag before instantiating lifecycle hooks; when disabled it falls back to raw `system_prompt_extras` vector to avoid regressions.  
- Progress log writer + telemetry injection guard on the same config, ensuring no sensitive data is stored unless the operator opts in.  
- Flag state is surfaced via `goose status` so users know whether refinements are active when debugging.
- **Implementation**: Added `PromptRefinementMode` (`crates/goose/src/config/prompt_refinement.rs`), `Config::prompt_refinement_mode` reader, CLI override `--prompt-refinement`, and session builder wiring so `Agent::configure_prompt_persistence` only activates when mode is `persist-only` or `enabled` (`crates/goose-cli/src/cli.rs:1000`, `crates/goose-cli/src/session/builder.rs:250-404`). Default behavior remains disabled, but the mode is logged at session start.

### Step: Write regression tests for prompt flows
**Status**: test plan drafted 2025-11-05  
**Context**: `crates/goose/tests/prompt_refinement.rs`

- **Unit**: add `PromptManager` tests that register/apply/retire instructions and assert snapshot-rendered prompts; use `insta` fixtures covering duplicates, overrides, and scope-specific behavior.  
- **Integration**: CLI session builder test ensures `--system-prompt-reset` plus `.goosehints` results in only Persistent instructions; mock `PromptHistoryWriter` to verify log entries.  
- **Server**: extend `/agent/prompt` route test to confirm repeated recipe updates refresh (not duplicate) instructions.  
- **Telemetry**: new async test verifying `ProviderUsage.instruction_hashes` is populated when the feature flag is on and omitted otherwise.  
- **Persistence**: simulate resume by serializing `PromptStateSnapshot`, reloading, and checking that Additional Instructions remain identical.

## Phase 4 – Validation

### Step: Run goose-self-test with refinements
**Status**: pending implementation availability  
**Context**: `just goose-self-test`

- Execute the recipe twice: once with `GOOSE_PROMPT_REFINEMENT=disabled` (baseline) and once with `enabled`.  
- Compare `prompt-history/*.jsonl` artifacts to ensure instruction stacks progress as expected (no duplication, overrides recorded).  
- Capture token metrics + success verdicts to confirm no regressions in test phases (file edits, tool usage, MCP).  
- Store the comparison summary under `thoughts/shared/research/2025-11-04-system-prompt-refinement-progress.md` for future runs.

### Step: Gather token telemetry baseline comparisons
**Status**: pending implementation availability  
**Context**: SessionManager analytics

- Export `session_tokens` + instruction counters via `goose session insights --format json`.  
- Compute deltas per phase (before vs after enabling refinements) to quantify overhead; target <1% token increase.  
- Feed results into `thoughts/shared/research/2025-11-04-system-prompt-refinement-progress.md` along with charts produced by `scripts/bench-postprocess-scripts/generate_leaderboard.py`.  
- Use findings to tune compaction thresholds if instruction scope bleed causes token bloat.

### Step: Review persistence artifacts for accuracy
**Status**: pending implementation availability  
**Context**: `prompt-history/` + `prompt-state/`

- Spot-check JSONL entries to ensure `order` increments, hashes match actual content (`sha256sum`), and scope transitions align with lifecycle events.  
- Validate snapshot reload by killing/restarting a session and verifying Additional Instructions stay in sync with the on-disk state (no dupes/missing entries).  
- Confirm `.goosehints` imports show up with `source.kind = "hint"` and the correct relative path recorded.  
- Document issues + fixes in this progress log.

### Step: Document workflow in thoughts records
**Status**: blocked on validation steps  
**Context**: future `thoughts/shared/research/...` entries

- Draft a how-to covering: enabling the feature flag, inspecting instruction stacks, resetting prompts, and interpreting telemetry charts.  
- Link to `AGENTS.md` under “Development Loop” so others know to update `goose-self-test` before merging prompt changes.  
- Include failure modes + mitigation (e.g., if logs fail to write) plus references to new CLI flags.  
- Publish once the validation steps confirm behavior.

### Step: Outline next session handoff tasks
**Status**: pending completion of above checkpoints  
**Context**: to be appended before sign-off

- Summarize remaining implementation tasks (life-cycle API wiring, tests, validation) plus pointers to relevant files.  
- Note any outstanding approvals (security, compliance) and link to tickets/slack threads.  
- Provide recommended order of operations for the next engineer (enable flag in staging, rerun tests, review telemetry).  
- Ensure handoff sits at the end of this document for quick discovery.
