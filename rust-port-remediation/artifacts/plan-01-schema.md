# Plan 01 — Canonical Schema & Shared-Type Unification

Agent 01 · Wave 0 READ-ONLY planning · branch `fix/audit-remediation`
Domain: canonical `oc-schema` types and promotion of every duplicate/schema-mirror type.
Evidence: static inspection of `crates/*/src`, `reference/packages/schema/src/`, audit reports
`01-architecture-modularity.md`, `18-testing-verification.md`, `FINDINGS.json`, `artifacts/01-duplicate-types.txt`.

---

## 1. Owned finding IDs

- **ARCH-001** (Critical) — Declared dependency graph is vestigial: zero `use oc_*` in production source.
  (Owned for the *type-unification* half; the wiring half belongs to Agent 02.)
- **ARCH-004** (High) — Canonical shared types unused; identical domain types defined 2–8× across crates.
- **ARCH-005** (High) — oc-client/oc-tui/oc-server/oc-session ship local DTO mirrors marked
  `TODO(integration): promote to oc-schema`.
- **ARCH-012** (Medium) — V1+V2 session model implemented twice (oc-schema vs oc-session v1.rs/v2.rs);
  schema crate unused by production code.
- **ARCH-014** (Low) — oc-util ripgrep types marked promote-to-oc-schema where the target already exists.
- **TEST-002** (High) — oc-session / oc-session-runner test only local mirror types; cross-crate wire
  consistency unproven.
- Also owns: the duplicate-type inventory (`artifacts/01-duplicate-types.txt`) and the TEST-002 evidence in
  report 18 (TEST-01). Supporting consolidated: INTEGRATION-001 (type-promotion portion), TEST-001
  (cross-crate serialization test portion).

## 2. Verdict and guiding principle

oc-schema is **already the correct, nearly complete canonical port** of `packages/schema` (lib.rs lists all
61 modules; `v1/session.rs` 1367 lines, `session_message.rs`, `session.rs`, `prompt.rs`, `model.rs`,
`filesystem.rs`, `session_event.rs`, `permission.rs`, `question.rs`, `location.rs`, `session_input.rs`,
`llm.rs` all exist). The remediation is therefore **delete + switch**, not *re-author*: move mirrors, keep
re-export shims during the transition, and fix the few *semantic* divergences the mirrors introduced
(String ISO times vs epoch-millis, `serde_json::Map` sorted keys vs `IndexMap`, `f64` vs `Finite`,
stringly `type_` vs typed enums).

**Critical correction to the audit inventory:** the "Message ×8 / Entry ×7 / Prompt ×8 / Shell ×4 /
Source ×5" counts conflate name collisions with true duplicates. Only these are real duplicates of
`@opencode-ai/schema` types. The rest are distinct models that must **not** be unified (see §5).

There are **three canonical homes**, mirroring the reference package boundaries:
1. `oc-schema` — `packages/schema` (session message/part/session/model/prompt/permission/question/location/
   input/event/filesystem/llm). THE FOUNDATION.
2. `oc-llm` — `packages/llm` (`schema/messages.ts`, `schema/events.ts`, `options.ts`).
3. `oc-client` — `packages/protocol` RPC envelopes *only if* they are not already in `packages/schema`
   (see §6 decision).

## 3. Canonical representation decisions (per type family)

| Type family | Canonical home (Rust) | Reference source | Mirrors to delete/switch |
|---|---|---|---|
| V2 `Session.Message` union + all variants (`AgentSwitched…Compaction`, `AssistantContent/Tool/Text/Reasoning`, `ToolState*`, `UnknownError`, `TokenUsage/TokenCache`, `TimeCreated/TimeCompleted`) | `oc_schema::session_message` | `packages/schema/src/session-message.ts` | oc-session `v2.rs`; oc-session-runner `session/message.rs`; oc-client `types/session_message.rs`; oc-acp `sdk.rs` (V1-flavored, map per variant) |
| `ToolContent/ToolTextContent/ToolFileContent/ProviderMetadata` | `oc_schema::llm` | `packages/schema/src/llm.ts` (imported by `packages/llm` per reference) | oc-llm `schema/messages.rs`; oc-session `v2.rs`; oc-session-runner `session/message.rs`+`llm/event.rs`; oc-tool `model.rs`; oc-client `types/session_message.rs` |
| V1 `SessionV1.Message`/`Part` (12 variants)/`WithParts`/`SessionSummary`/`SessionTokens`/`SessionInfo`/`SessionTime`/`FileDiff`/`PermissionRule`/named `Error`/`OutputFormat` | `oc_schema::v1::session` | `packages/schema/src/v1/session.ts` | oc-session `v1.rs`; oc-tui `types.rs`; oc-cli `cli/cmd/run/types.rs`; oc-acp `sdk.rs` (Part/Message portion) |
| V2 `Session.Info` + `Time`/`TokenUsage` | `oc_schema::session` | `packages/schema/src/session.ts` | oc-server `schema.rs`; oc-client `types/session.rs`; oc-session `session.rs` (`Info` alias); oc-acp `sdk.rs` (`Session`); oc-core `agent.rs`/`integration.rs` where applicable |
| `Model.Ref` | `oc_schema::model` | `packages/schema/src/model.ts` | oc-server `schema.rs`; oc-session-runner `session/schema.rs`; oc-client `types/model.rs`; oc-acp `sdk.rs`; oc-tui `types.rs`; oc-core `agent.rs`; oc-session `v1.rs`(V1 local copy stays V1) |
| `Prompt`/`Source`/`FileAttachment`/`AgentAttachment` | `oc_schema::prompt` | `packages/schema/src/prompt.ts` | oc-session `v2.rs`; oc-session-runner `session/event.rs`; oc-client `types/prompt.rs`; oc-mcp `types.rs` (verify it is the schema Prompt, not a config shape) |
| `SessionEvent` union (`session.next.*`) | `oc_schema::session_event` | `packages/schema/src/session-event.ts` | oc-session `v2.rs::EventData`; oc-session-runner `session/event.rs` |
| `Event.Payload` envelope + `Durable` | `oc_schema::event` (**add concrete `Payload` struct**; Definition/inventory already present) | `packages/schema/src/event.ts` | oc-server `event.rs`; oc-sync `sync/event.rs`; oc-core `event.rs`; oc-session `v2.rs::Event` |
| `Location.Ref/Info/Project` | `oc_schema::location` | `packages/schema/src/location.ts` | oc-server `schema.rs`+`location.rs`; oc-core `location.rs`; oc-session-runner `session/schema.rs` (runtime `Location` with `owns()` stays a domain type — oc-core or runner, not schema) |
| `SessionInput.Admitted` | `oc_schema::session_input` | `packages/schema/src/session-input.ts` | oc-server `schema.rs`; oc-client `types/session_input.rs` |
| `Permission`/`Question`/`Agent`/`Skill`/`Command`/`Connection`/`Credential`/`Provider` | `oc_schema::{permission,question,agent,skill,command,connection,credential,provider}` | `packages/schema/src/*` | oc-client `types/*`; oc-command `question/schema.rs`; oc-acp `sdk.rs`/`agent.rs`; oc-server `handlers/permission.rs`; oc-provider `models_dev.rs` (catalog) |
| `FileSystem.Entry/Submatch/Match` | `oc_schema::filesystem` | `packages/schema/src/filesystem.ts` | oc-util `ripgrep/mod.rs`; oc-tool `ripgrep.rs` (3rd copy). Note: rg `FindInput` is process input, stays in oc-util |
| `LLM.Message`/parts/`ToolDefinition`/`LLMRequest`/`GenerationOptions` | `oc_llm::schema::messages` | `packages/llm/src/schema/messages.ts` | oc-session-runner `llm/message.rs`; oc-session `llm.rs` (subset, already marked `promote to oc-llm`) |
| `LLM.Events`/`Usage` | `oc_llm::schema::events` | `packages/llm/src/schema/events.ts` | oc-session `llm.rs` |
| `Session.Message` (AI-SDK legacy service model) | **stays in oc-session** (`message.rs`) | `packages/opencode/src/session/message.ts` | none (domain-owned; only wire-boundary maps to canonical) |
| `UiMessage/UiPart` (V2 session UI model) | **stays in oc-session** (`message_v2.rs`) | `core/session` | none |

## 4. Files expected to change (per crate)

Read-only estimate; each row = a file whose local mirror types are replaced by `oc_schema::`/`oc_llm::`
imports (or that adds the dependency / re-export shim). ~85 files across 16 crates.

- **oc-schema** (~6, additive): `src/lib.rs` (re-exports); `src/event.rs` (add concrete `Payload`
  envelope struct); `src/v1/session.rs` + `src/session_message.rs` (add ergonomic helper impls
  `id()`/`role()`/`id_session()`/`as_tool()` so consumers keep their convenience methods); new golden
  tests `tests/promoted_types.rs`.
- **oc-util** (2): `src/ripgrep/mod.rs` (delete `Entry`/`Submatch`/`Match`, import `oc_schema::filesystem`);
  `Cargo.toml` (add `oc-schema`).
- **oc-tool** (2): `src/ripgrep.rs` (delete 3rd copy of Entry/Match/Submatch); `src/model.rs` (ToolContent →
  `oc_schema::llm`).
- **oc-llm** (1): `src/schema/messages.rs` (delete local `ToolTextContent`/`ToolFileContent`/`ToolContent`,
  `pub use oc_schema::llm::*` per reference import pattern).
- **oc-session** (~12): `src/v1.rs`, `src/v2.rs`, `src/store.rs`, `src/session.rs`, `src/message_updater.rs`,
  `src/revert.rs`, `src/compaction.rs`, `src/compaction_core.rs`, `src/processor.rs`, `src/llm.rs` (→ oc-llm),
  `src/prompt.rs`, `src/todo.rs`, `src/summary.rs`, `src/status.rs` (type switches; v1.rs/v2.rs become
  re-export shims then deletions).
- **oc-session-runner** (~8): `src/llm/message.rs`, `src/llm/event.rs`, `src/session/message.rs`,
  `src/session/schema.rs`, `src/session/event.rs`, `src/runner/*`, `src/retry.rs`; `Cargo.toml` (add `oc-schema`).
- **oc-server** (~8): `src/schema.rs`, `src/event.rs`, `src/location.rs`, `src/state.rs`,
  `src/instance_handlers.rs`, `src/handlers/session.rs`, `src/handlers/permission.rs`, `src/route.rs`.
- **oc-client** (~25): `src/types/mod.rs` + 23 `src/types/*.rs` modules (promote or re-export from
  oc-schema); `src/generated.rs`; `src/lib.rs` (keep RPC transport in `client.rs`).
- **oc-acp** (~4): `src/sdk.rs`, `src/usage.rs`, `src/agent.rs`, `src/types.rs` (types already in
  oc-schema only; **ACP wire types in `types.rs` stay local**).
- **oc-tui** (~3): `src/types.rs`, `src/client.rs`, `src/app.rs` (type refs).
- **oc-core** (~5): `src/event.rs`, `src/location.rs`, `src/integration.rs`, `src/agent.rs`, `src/process.rs`.
- **oc-provider** (~3): `src/models_dev.rs`, `src/provider/mod.rs`, `src/provider/auth.rs`.
- **oc-sync** (1): `src/sync/event.rs`.
- **oc-mcp** (2): `src/types.rs` (verify/switch Prompt), `src/config.rs` (marker cleanup).
- **oc-command** (1): `src/question/schema.rs`.
- **oc-cli** (~4): `src/cli/cmd/run/types.rs`, `src/cli/cmd/run/client.rs`, `src/cli/cmd/stats.rs`,
  `src/cli/cmd/models_dev.rs`.
- **oc-config / oc-plugin**: no type promotion (only name-collision documentation; plugin stays `Value`-typed).

## 5. Name collisions that must NOT be unified (do not touch)

These share a name with an oc-schema type but are **different models** — promoting them would corrupt
wire format or conflate protocols:
- `Message`: oc-util `util/rpc.rs`, oc-mcp `jsonrpc.rs` (JSON-RPC/SSE-RPC envelopes); oc-llm
  `schema/messages.rs::Message` (LLM.Message, canonical home oc-llm).
- `Entry`: oc-plugin `meta.rs`, oc-mcp `auth.rs`, oc-session `compaction_core.rs`, oc-config `v1/lsp.rs`
  (metadata/auth/compaction/LSP entries — not filesystem entries).
- `Source`: oc-command `command/mod.rs`, oc-config `variable.rs`, oc-provider `provider/mod.rs`
  (command/config/provider sources).
- `Shell`: oc-util `util/process.rs` (process shell settings).
- `Prompt`: oc-command `question/schema.rs` (QuestionV1 prompt), oc-core `integration.rs`,
  oc-provider `provider/auth.rs` (verify individually; may be genuine).
- `Provider`: oc-config `v1/provider.rs` (config provider vs registry provider).
- `Model`: oc-config `v1/provider.rs`, oc-provider `models_dev.rs` vs `oc_schema::model`.
- oc-acp `types.rs` (ContentBlock, ToolCall, McpServer, PermissionOption …) — ACP protocol, distinct spec.

## 6. Protocol DTO decision (client/server)

`oc-server::schema` and `oc-client::types` both re-implement `packages/protocol` envelopes
(`SessionsResponse`, `SessionMessagesResponse`, cursors, `HealthOutput`, input DTOs). Per report-01
ARCH-005 convention these are marked "promote to oc-schema". **Recommendation:** promote them into
oc-schema (module `oc_schema::protocol` or folded into the matching schema module) because both consumers
already depend on oc-schema and there is no `oc-protocol` crate; flag a future `oc-protocol` split as the
reference-faithful alternative. This is the one open decision for the coordinator.

## 7. TYPE-PROMOTION.csv rows (estimate)

~35 rows consolidating ~90 local type definitions. `to` is always `oc-schema` unless noted `oc-llm`.

| # | from (crate::module) | to (canonical) | consumers to switch | adapter reason |
|---|---|---|---|---|
| 1 | oc-session::v2::{Message,AgentSwitched,ModelSwitched,User,Synthetic,System,Shell,Compaction,Assistant,…} | oc_schema::session_message | oc-session store/message_v2/processor/compaction | f64→Finite, u64→DateTimeUtc, String type_→enum, untagged/tag unification |
| 2 | oc-session-runner::session::message::{SessionMessage,Assistant,User,…,ToolState,UnknownError} | oc_schema::session_message + oc_schema::llm + oc_schema::prompt | runner runner/*, to_llm_message | **String ISO times→i64 epoch-millis**; serde_json::Map→IndexMap; add as_tool() |
| 3 | oc-client::types::session_message::{SessionMessage,…,ToolContent,Tokens,…} | oc_schema::session_message + llm | client.rs, tests/contract.rs | field/name alignment |
| 4 | oc-session::v2::ToolContent, oc-tool::model::ToolContent, oc-session-runner::llm::event::ToolContent | oc_schema::llm::ToolContent | session, tool, runner | delete + import |
| 5 | oc-llm::schema::messages::{ToolTextContent,ToolFileContent,ToolContent} | oc_schema::llm | oc-llm protocols/* | delete + import (matches reference `@opencode-ai/schema/llm`) |
| 6 | oc-session::v1::{Part,Info,WithParts,SessionInfo,SessionSummary,…,ToolState*,CacheTokens,ModelRef} | oc_schema::v1::session | oc-session store/revert/summary; **tests/roundtrip.rs** | f64→Finite, JsonMap→IndexMap, typed enums; move id()/role() helpers |
| 7 | oc-tui::types::{Message,Part,UserMessage,AssistantMessage,SessionSummary,SessionShare,ModelRef,PermissionRule,…} | oc_schema::v1::session | oc-tui client.rs/app.rs | shape alignment; TUI-only Todo/Status stay |
| 8 | oc-cli::run::types::{Part,MessageInfo,SessionInfo,ToolState,SessionStatus} | oc_schema::v1::session | run/client.rs | shape alignment |
| 9 | oc-acp::sdk::{Session,Message,Part,TextPart,FilePart,ReasoningPart,ToolPart,ToolState,…} | oc_schema::v1::session + session_message (per variant) | acp service.rs | mixed V1/V2 — verify each variant |
| 10 | oc-server::schema::{SessionInfo,ModelRef,LocationRef,LocationInfo,ProjectRef,Tokens,CacheTokens,SessionTime} | oc_schema::session + model + location + session_message | instance_handlers/handlers | f64→Finite, revert Value→revert::State |
| 11 | oc-session::session::{Info,SessionModelRef,Tokens,CacheTokens,Summary,SessionRow,Time} | oc_schema::session + session_message | store.rs, summary.rs, from_row/to_row | f64→Finite |
| 12 | oc-session-runner::session::schema::{SessionInfo,ModelRef,LocationRef} | oc_schema::session + model + location | runner/services | narrow projection → full type or From |
| 13 | oc-client::types::session::SessionInfo/SessionTokens/SessionLocation/SessionsResponse/ResponseCursor | oc_schema::session + protocol DTOs | client.rs | envelope alignment |
| 14 | oc-client::types::model::ModelRef | oc_schema::model | client.rs | delete + import |
| 15 | oc-client::types::prompt::Prompt | oc_schema::prompt | client.rs | delete + import |
| 16 | oc-client::types::permission, permission_saved | oc_schema::permission | client.rs, server handlers | enum/effect alignment |
| 17 | oc-client::types::question | oc_schema::question | client.rs | delete + import |
| 18 | oc-client::types::{agent,skill,command,connection,credential,project,project_copy,revert,reference,location,filesystem,event,pty,health,integration,schema,provider} | oc_schema::* | client.rs | delete + import |
| 19 | oc-acp::usage::SessionMessage | oc_schema::session_message | acp usage.rs | envelope alignment |
| 20 | oc-server::event::{Event,Durable} | oc_schema::event (add Payload) | sse.rs, bus | keep EventBus local |
| 21 | oc-sync::sync::event::{Event,Durable,DurableEnvelope} | oc_schema::event | control_plane | envelope alignment |
| 22 | oc-core::event::{Durable,DurableInfo,DurableRegistry} | oc_schema::event (Durable part) | bus.rs | registry stays domain-owned |
| 23 | oc-session::v2::Event/EventData | oc_schema::session_event | oc-session | tag/kebab-case alignment |
| 24 | oc-server::schema::Admitted, oc-client::types::session_input | oc_schema::session_input | server/client | name/field alignment |
| 25 | oc-util::ripgrep::{Entry,Submatch,Match} | oc_schema::filesystem | rg binary.rs, oc-tool grep | String type→EntryType; keep FindInput |
| 26 | oc-tool::ripgrep::{Entry,Match,Submatch} | oc_schema::filesystem | tool grep | delete + import |
| 27 | oc-session-runner::llm::message::{Message,ContentPart,SystemPart,…,LLMRequest,ToolDefinition} | **oc_llm::schema::messages** | runner runner/* | delete + import (verify subset) |
| 28 | oc-session::llm::{Usage,LLMEvent,ContentPart,…,ModelMessage} | **oc_llm::schema::{events,messages}** | oc-session | delete + import (subset) |
| 29 | oc-server::handlers::permission::{PermissionEffect,…} | oc_schema::permission | permission handler | effect enum alignment |
| 30 | oc-command::question::schema | oc_schema::question (+v1) | command question | shape alignment |
| 31 | oc-core::{agent::ModelRef?, integration::Prompt, location, process::Command} | oc_schema::{model,prompt,location,command} | oc-core | per-type alignment |
| 32 | oc-provider::models_dev::Model | oc_schema::model (catalog info) | models_dev.rs | Info fields alignment |
| 33 | oc-mcp::types::Prompt (if schema shape) | oc_schema::prompt | mcp | verify |
| 34 | oc-cli::stats::Tokens | oc_schema::session_message::TokenUsage | stats.rs | delete + import |
| 35 | oc-acp::sdk::{Config,ProviderInfo,ModelInfo,AgentInfo,SkillInfo,CommandInfo,Permission…} | oc_schema::{provider,model,agent,skill,command,permission} | acp | per-type alignment |

## 8. Dependencies on other agents

- **Agent 02 (composition root)** — I am a hard prerequisite: Agent 02's `LocalClient`/`Server::listen`
  wiring consumes `oc_schema` types. Sequence: my promotion lands first; Agent 02 branches after. I must
  ship re-export shims so Agent 02 can code against stable canonical paths immediately.
- **Agent 18 (tests)** — bidirectional: I depend on Agent 18 to add the cross-crate serialization harness
  and to close TEST-002 (retarget oc-session/runner tests at oc_schema); Agent 18 depends on my canonical
  types being stable + the `promoted_types` goldens.
- **Agent 06 (session), Agent 07 (runner), Agent 10 (server), Agent 11 (client), Agent 16 (TUI)** — all
  consume promoted types; coordinate file ownership (e.g. oc-server `schema.rs` is touched by Agent 10;
  oc-client `types/*` by Agent 11). Agree a single "promotion wave" PR set to avoid conflicts.
- **Agent 03 (database)** — `from_row/to_row` in oc-session `session.rs` convert `SessionRow ↔ SessionInfo`;
  keep DB DDL untouched, only the Rust struct binding changes.
- **Agent 20 (release/CI)** — add `cargo machete`/grep guards after promotion.

## 9. Proposed serialization / compile-time tests

1. `oc-schema/tests/promoted_types.rs` — golden byte-exact serialization for every promoted type family,
   fixtures transcribed from reference zod output (epoch-millis times, `IndexMap` key order, `Finite`
   number formatting, `null` vs omitted).
2. Cross-crate equivalence test (Agent 18): construct one value via each legacy mirror, serialize, and
   assert byte-identity with the canonical `oc_schema` serialization — the current missing harness that
   TEST-002 demands.
3. V1-vs-V2 isolation test: a V1 `Part`/`Info` and a V2 `Session.Message` are distinct types; assert no
   type-name `From` exists across the boundary and that each serializes with its own tag set
   (`role`/`type`-string vs `status`/`type`-enum).
4. `cargo check -p <every consumer>` green after promotion (compile-time proof of switchover).
5. CI grep guards: zero `TODO(integration): promote to oc-schema` in `crates/*/src`; zero `pub struct`
   names colliding with oc-schema types outside oc-schema; zero `serde_json::Map` on promoted wire types
   (key-order guard).

## 10. Risks

1. **V1/V2 conflation (highest)** — same-named types (`Message`, `Part`, `ToolState`, `User`, `Assistant`,
   `Compaction`, `SessionInfo`, `ModelRef`) exist in V1 and V2 with different serialized shapes (V1: tagged
   `role`, snake part types, `sessionID/messageID`, f64 fields; V2: `type` enum, `time.created` epoch-millis,
   `content` arrays). A mechanical rename would silently swap wire formats. Mitigation: namespaced canonical
   paths (`oc_schema::v1::*` vs `oc_schema::*`), per-version goldens, no shared `From`.
2. **Key order + nullability drift** — consumer mirrors use `serde_json::Map` (BTreeMap = sorted keys,
   `preserve_order` enabled only in oc-config/oc-core/oc-llm) and `skip_serializing_if = "String::is_empty"`
   defaults, while oc-schema uses `IndexMap` insertion order, `Option` omitted-when-None, and `Finite`
   (JS `Number.toString` formatting). Promoting without re-verifying bytes breaks rule-1 parity.
3. **Cross-crate blast radius / merge ordering** — ~85 files across 16 crates, many also owned by other
   agents. If promotion lands late or conflicts, integration stalls. Mitigation: re-export shims, bottom-up
   dependency-order PRs, land before Agent 02.

## 11. Recommended merge order

**Yes — this is the first Wave-1 item**, ahead of broad integration. Every other wiring path consumes these
types. Sequence:
1. `oc-schema` canonical additions + `promoted_types` goldens (isolated, no consumer churn).
2. Per-domain promotion PRs in bottom-up dependency order: filesystem (`oc-util`→`oc-tool`) →
   `oc-llm` ToolContent + LLM schema → `oc-session` v1/v2 → `oc-session-runner` → `oc-server`/`oc-client`/
   `oc-acp`/`oc-tui`/`oc-core`/`oc-sync` → `oc-cli`.
3. Only then does Agent 02 broad integration begin, followed by Agent 18 cross-crate serialization tests.

## 12. Acceptance checklist (this plan's scope)

- [ ] `grep -r "TODO(integration): promote to oc-schema" crates/*/src` → 0 matches.
- [ ] Every consumer listed in §4 imports `oc_schema::`/`oc_llm::` (grep-count > 0 per crate).
- [ ] `cargo build --workspace` and `cargo test --workspace` green, including oc-session `roundtrip.rs`
      and oc-session-runner `runner_loop.rs` now exercising canonical types (TEST-002 closed).
- [ ] `oc-schema/tests/promoted_types.rs` byte-goldens pass for all 35 promotion rows.
- [ ] Duplicate-type inventory shrinks to canonical counts: SessionInfo 7→1, ModelRef 7→1,
      filesystem Entry 3→1, V2 Message copies → 1, ToolContent 6→1.
- [ ] V1/V2 isolation test passes (no cross-version `From`).
- [ ] Key-order guard: no `serde_json::Map` on promoted wire types.
- [ ] `cargo machete`/CI grep guard clean (no new mirrors).
- [ ] oc-util/oc-plugin Cargo.toml gains `oc-schema` only where truly needed (oc-plugin stays `Value`-typed).
- [ ] oc-acp ACP wire types (`types.rs`) and JSON-RPC `Message` envelopes untouched.
