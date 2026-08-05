# Plan 08 — Permission Service and Authorization Boundary (Agent 08)

Domain: permission service and the authorization boundary. Merge gate: **SEC-001 must land
before tool execution is reachable from the binary** (blocking critical).

Status: WAVE-0 PLANNING (read-only). No production source modified.

---

## 1. Owned findings

| ID | Severity | Title | Status |
|---|---|---|---|
| SEC-001 | Critical (release blocker) | Tool approval gate is record-only; model tool calls execute without approval | owned |
| TOOLS-02 | High (release blocker, shared w/ Agent 11/09) | Permission/approval gate is a record-only stub | co-owned (enforcement side = this plan) |
| TOOLS-06 | Medium (parity debt) | `collect()` tokenizer approximates tree-sitter; segment boundaries diverge → approval prompt binds to wrong command text | co-owned (prompt binding, see §6.5) |
| SEC-006 | Low/Med (plugin) | Plugin host `tool_ask` defaults to allow | interface depends on this service |

Evidence (read): `rust-port-audit/13-security-threat-model.md` SEC-002; `11-tool-execution-filesystem.md`
TOOLS-02; `FINDINGS.json` SEC-001.

### 1.1 What is broken today (verified static)

- `crates/oc-tool/src/model.rs:386-389` — `ToolContext::ask` pushes to `ctx.asks` and returns `Ok`.
  Never evaluated, never prompts, never blocks.
- `crates/oc-tool/src/core/tool.rs:42-45` — `CoreContext::assert` pushes to `ctx.asks`, returns `Ok`.
- `crates/oc-server/src/handlers/permission.rs:33-63` — `session_permission_create` stores the body and
  returns `effect:"allow"` unconditionally; `session_permission_reply` (104-125) removes the stored
  request and returns `204` without resolving any waiter.
- `crates/oc-session/src/processor.rs:69-77` — `ProcessorDeps::ask_permission` is declared but never
  called; `FakeDeps` returns `Ok(())` (processor.rs:862).
- `crates/oc-session-runner/src/session/services.rs:283-297` — `ToolSettle` has no permission hook;
  `runner/llm.rs:529-557` settles every local tool call unconditionally.
- `crates/oc-session-runner/src/runner/llm.rs:486-528` — settle path performs no rule evaluation.
- `crates/oc-tool/src/core/registry.rs:258-267` — only `deny *`-style materialization filtering exists.
- `crates/oc-cli/src/cli/cmd/run/events.rs:301-327` — `permission.asked` handler exists and replies
  once/reject, but nothing ever *emits* the event because nothing blocks or asks.
- `crates/oc-server/src/event.rs:66-68` — `permission_id()` mints `perm_...`; reference uses `per_...`
  (`isStartsWith("per")` passes, but not byte-identical → parity debt).
- `crates/oc-server/src/state.rs:29` — `stores.permissions: HashMap<String, Value>` is a blind store,
  not a pending registry; no waiter association, no save persistence, no project-scoped saved rules.

Reachability caveat (unchanged from audit): the runner/serve seams are still `TODO(integration)`
(TOOLS-01 / INTEGRATION-001), so this is currently unreachable from the binary. The instant Agent
02/07 wiring lands without this fix, the control becomes exploitable (prompt-injected repo →
arbitrary bash/write/webfetch/apply_patch). Hence the merge gate.

---

## 2. Reference semantics to replicate (from `reference/`)

Two permission engines exist in the reference; both must be ported:

### 2.1 V1 `Permission.Service` (`packages/opencode/src/permission/index.ts`)

- `evaluate(permission, pattern, ...rulesets)` → last matching rule across merged `rulesets`
  (`.findLast`, matching Rust `oc-session::permission::evaluate` which uses `.rev().find(...)`), else
  default `{permission, pattern:"*", action:"ask"}`. `index.ts:28-38`.
- `ask(input)`: for each `request.patterns` evaluate against `ruleset ++ approved` (in-memory,
  session-scoped approved list):
  - `deny` → return `DeniedError` (fail the effect; no side effect runs).
  - `allow` → continue; if **no** pattern needs ask → return immediately (no prompt).
  - else create pending, publish `permission.asked`, **block** `Deferred.await(deferred)`.
    `index.ts:67-107`.
- `reply` (`index.ts:109-167`):
  - `reject` → fail deferred with `CorrectedError{feedback}` if message, else `RejectedError`; then
    **cascade**: reject every other pending request in the same session (fail + publish
    `permission.replied reject`).
  - `once` → succeed deferred; no rule saved.
  - `always` → push each `request.always` pattern into `approved` as
    `{permission, pattern, action:"allow"}`; then cascade-approve other same-session pendings whose
    every pattern now evaluates to `allow` under `approved` alone (publish `permission.replied always`).
- `fromConfig` / `expand`: agent config permission `{permission: action}` → `pattern:"*"`;
    `{permission:{pattern: action}}` → expand `~`/`$HOME` (index.ts:178-198).
- `disabled`/`visibleTools`: whole-tool hiding when a `deny *` rule matches the tool's permission
  (edit/write/apply_patch map to `edit`; mcp resource tools map to `read`). index.ts:204-219.

### 2.2 V2 `PermissionV2.Service` (`packages/core/src/permission.ts`)

- Model differs: `{action, resources, save, metadata, source}` instead of
  `{permission, patterns, always}`. `save` = the resource patterns to persist on an "always" reply.
- `ask(input)`: evaluate rules = `configured(session)` = `agent.permissions ?? [{action:"*",resource:"*",effect:"deny"}]`
  (**fail-closed default for missing agent permissions**); `denied()` if any resource evaluates deny →
  `{effect:"deny"}`; else evaluate against `rules ++ savedRules()`; effect = deny if any, else ask if
  any, else allow. Returns `{id, effect}` immediately, recording the pending only when `ask`.
  permission.ts:155-195.
- `assert(input)`: like `ask` but **blocks** when effect is `ask` (uninterruptible mask →
  `Deferred.await(item.deferred)`), fails `BlockedError{rules}` on deny; maps user decline to
  `DeclinedError` / `CorrectedError{feedback}` (permission.ts:197-218).
- `reply` (permission.ts:220-286): `reject` → fail + **cascade reject same session**; `always` with
  non-empty `save` → **persist each save resource** via `PermissionSaved.add` (project-scoped,
  unique index) then cascade-approve same-session pendings now fully allowed.
- Saved decisions: `packages/core/src/permission/saved.ts` — `list(projectID)` / `add({projectID,action,resources})`
  / `remove(id)` over the `permission` table
  (`packages/core/src/permission/sql.ts`: `id`, `project_id` FK cascade, `action`, `resource`,
  `UNIQUE(project_id, action, resource)`). Saved rules evaluate as `effect:"allow"`.
- Tool-side binding: leaves call `permission.assert({... , sessionID, agent, source:{type:"tool", messageID, callID}})`
  **inside** `execute`, so approval is bound to the exact message + callID that produced the call
  (see `packages/core/src/tool/read.ts:55-79`, `write.ts:65-85`). The Rust V2 leaves already emit
  the same `CorePermissionSource{message_id, call_id}` (core/write.rs:88-102, core/read.rs:95-115) —
  the missing half is the blocking evaluation.

### 2.3 Prompt delivery (server → client → reply)

- `PermissionV2.ask` publishes `permission.asked` (Request-shaped) on the event bus
  (`EventV2Bridge.publish`). Clients subscribe over SSE (`event.subscribe()`).
- TUI/interactive: `cli/cmd/run/footer.permission.tsx` + `permission.shared.ts` render
  once/always/reject and POST `client.permission.reply({requestID, reply, message})`.
- Non-interactive `run` (`cli/cmd/run.ts:796-815`): on `permission.asked`, `-y/--auto` →
  reply `once`; otherwise print "permission requested: … auto-rejecting" → reply `reject`.
  The Rust `run/events.rs:301-327` already mirrors this client-side logic — it only needs the server
  side to actually ask.
- ACP: `packages/opencode/src/acp/permission.ts` bridges `requestPermission` (agent-side) and replies
  once/always/reject; a missing `requestPermission` capability → auto-reject.
- `Question` service (`src/question/index.ts`) is the sibling pattern: server-side `pending` map +
  `Deferred`, publish `Asked`, await, `reply`/`reject` resolve. The permission service should mirror
  its shape (single pending registry per location/session, finalizer rejects all pending on teardown).

---

## 3. Files to change

Owned crates (`oc-session` canonical, `oc-server` handlers, `oc-tool` context hooks, `oc-session-runner` seam):

| File | Change | Notes |
|---|---|---|
| `crates/oc-session/src/permission.rs` | Promote to the canonical V1 service: `evaluate`, `merge`, `from_config`, `disabled`, `Service` (pending registry + deferreds + reply cascade). | Keep pure fns; add blocking `ask`. |
| `crates/oc-session/src/permission_v2.rs` (new) | V2 service: `ask` (non-blocking, returns effect) + `assert` (blocking) + `reply` + `evaluate(action,resource,rulesets)` + `configured()` fail-closed default. | Mirrors `core/src/permission.ts`. |
| `crates/oc-session/src/permission/saved.rs` (new) | `PermissionSaved` service: `list`/`add`/`remove` over oc-database `permission` table (or injected store trait). | §5 persistence. |
| `crates/oc-server/src/handlers/permission.rs` | `session_permission_create` → call V2 `ask`, return real `{id, effect}` (allow/deny/ask); `session_permission_reply` → call service `reply` (resolve deferred / cascade); `permission_saved_list`/`remove` → read/write saved store. | Keep wire shape `PermissionCreateData{data:{id,effect}}`, `ReplyBody{reply,message}`. |
| `crates/oc-server/src/event.rs` | `permission_id()` prefix `perm_` → `per_`; wire `permission.asked`/`permission.replied` (and `permission.v2.*`) emission from the service via the injected `EventBus`. | Parity fix (2.3). |
| `crates/oc-tool/src/model.rs` | `ToolContext::ask` becomes async, delegate to injected `Arc<dyn PermissionService>`; keep `asks` recording for compat only. `ToolContext` gains the service handle (replaces `NullServices` ask). | V1 hook (3.1). |
| `crates/oc-tool/src/core/tool.rs` | `CoreContext::assert` async → injected V2 service `assert`; `CoreContext` gains `Arc<dyn PermissionV2Service>`. `CoreTool::execute`/`make` must become async-capable (`BoxFuture` execute, `assert` awaits) so suspension is legal. | V2 hook (3.2). |
| `crates/oc-session-runner/src/session/services.rs` | Add `PermissionService` (V1) to `RunnerDeps`/settlement; `ToolSettle` impl for permission-aware settle. | Runner seam. |
| `crates/oc-session-runner/src/runner/llm.rs` | Settle path: evaluate before executing; map `DeniedError`/`BlockedError`/`Declined`/`Corrected` to `ToolSettlementError::Declined` + `continue_loop_on_deny` handling. | 486-557 block. |
| `crates/oc-session/src/processor.rs` | Wire `ProcessorDeps::ask_permission` (blocking V1 ask) at the tool-execution boundary. | 69-77 seam. |
| `crates/oc-session/src/v1.rs` | `PermissionRule` action as enum `allow/deny/ask` (string today). | Optional normalization. |

> Ownership note (CONTEXT.md rule 5): files above touch `oc-tool`/`oc-session-runner`/`oc-server`,
> which are owned by Agents 09/07/10. The permission *service* (oc-session) is canonical here; the
> `ToolContext`/`CoreContext`/`ToolSettle` hook changes must be agreed with Agents 07/09 and land in
> the same merge window as this service. Where a type is missing in `oc-schema`, define a private
> mirror marked `TODO(integration)` per the contract.

---

## 4. Permission service API contract (canonical, oc-session)

```rust
// V1 — used by the V1 ToolContext.ask path (mirrors reference Permission.Service).
pub trait PermissionV1Service: Send + Sync {
    fn ask(&self, input: AskInputV1)
        -> Pin<Box<dyn Future<Output = Result<(), PermissionV1Error>> + Send + '_>>; // blocks on "ask"
    fn reply(&self, input: ReplyInputV1) -> Pin<Box<dyn Future<Output = Result<(), NotFound>> + Send + '_>>;
    fn list(&self) -> Vec<RequestV1>;
}
// AskInputV1 = { session_id, permission, patterns, metadata, always, ruleset, tool }
// errors: Denied(ruleset) | Rejected | Corrected(feedback)

// V2 — used by CoreContext.assert / server POST (mirrors reference PermissionV2.Service).
pub trait PermissionService: Send + Sync {
    fn ask(&self, input: AskInput)
        -> Pin<Box<dyn Future<Output = Result<AskResult, SessionNotFound>> + Send + '_>>; // returns {id,effect} immediately, records pending on "ask"
    fn assert(&self, input: AskInput)
        -> Pin<Box<dyn Future<Output = Result<(), PermissionError>> + Send + '_>>;        // blocks on "ask"; deny -> Blocked{rules}
    fn reply(&self, input: ReplyInput)
        -> Pin<Box<dyn Future<Output = Result<(), NotFound>> + Send + '_>>;               // resolves deferred + cascades + persists save
    fn get(&self, id: &str) -> Option<Request>;
    fn list(&self) -> Vec<Request>;
}
// AskInput = { session_id, action, resources, save, metadata, source, agent }
// AskResult = { id, effect: allow|deny|ask }
// errors: Blocked(rules) | Declined | Corrected(feedback)
```

State (one per active project/session, shared by all sessions in the instance):
- `pending: Mutex<HashMap<RequestID, Pending>>`, `Pending { request, deferred: oneshot::Sender<Reply> }`.
- `approved: Mutex<Vec<Rule>>` (V1 in-memory "always" rules for the process lifetime).
- `saved: Arc<dyn PermissionSaved>` (V2 durable rules, project-scoped).
- `events: Arc<EventBus>`-like publisher for `permission.asked` / `permission.replied`.
- Teardown finalizer: fail all pending (equivalent of reference `addFinalizer` + ACP auto-reject).

Rule evaluation (both engines): merged rulesets, **last match wins**, default `ask` (V1) / from
agent-permission default `deny` when absent (V2). Wildcard matching reuses the existing
`oc-session::permission::wildcard::matches` / `oc-tool::util::wildcard_match` (regex-compiled,
linear time — no ReDoS).

---

## 5. Ask / suspend / reply protocol (server + question round trip)

1. Runner fiber settles a tool call (`oc-session-runner` `ToolSettle` / V1 processor).
2. Tool leaf `execute` calls `ctx.assert`/`ctx.ask` with `source{messageID, callID}` (already emitted
   by the V2 leaves) and `session_id`, `action`/`permission`, `resources`/`patterns`, `save`/`always`.
3. Service evaluates; on `allow` → returns `Ok` immediately (no prompt, side effect proceeds).
   On `deny` → returns `Blocked`/`Denied` (side effect never runs).
4. On `ask`: service inserts `Pending`, publishes `permission.asked` (Request-shaped payload) to the
   server `EventBus` (SSE subscribers receive it), and **suspends** the calling future on the
   `oneshot` receiver. The runner fiber parks; the tool never reaches its fs/shell/network effect.
5. Client (TUI footer / non-interactive run loop / ACP bridge / SDK user) POSTs
   `POST /api/session/:sessionID/permission/:requestID/reply` with `{reply, message}`.
6. `session_permission_reply` calls `PermissionService::reply`: resolves the `oneshot` with
   `once`/`always`/`reject`, performs the cascade (same-session pendings), persists `save` rules on
   `always`, publishes `permission.replied`.
7. Suspended future resumes: `once`/`always` → `Ok`, proceed with the tool effect; `reject` →
   `Declined`/`Corrected{feedback}` surfaced to the model as the tool result (matching reference
   error-to-tool-result mapping) or `ToolSettlementError::Declined`.
8. Abort/interrupt path: runner cancellation fails the settlement; pending entries are failed by the
   teardown finalizer so no waiter leaks.

Async legality: V1 tool `execute` is already `BoxFuture` (`tool/tool.rs:15-22`), so `ToolContext::ask`
can await. V2 `CoreTool::execute` is currently a *sync* closure boxed into a future run with
`run_future`/`block_on` (`core/tool.rs:203-223`) — **this cannot suspend**. Agent 07 must convert V2
`make` to an async-execute form (like `def_async`) and drop `block_on` inside async contexts so
`assert` awaiting a pending reply is legal inside the runner fiber.

---

## 6. Persistence of saved decisions

- Durable "always" rules are V2-only (`save` + `always` replies). The `permission` table already
  exists in oc-database (`crates/oc-database/src/schema.rs:110-119,280`; migration
  `m20260602002951_lowly_union_jack.rs`) with `id`, `project_id` (FK, cascade), `action`, `resource`,
  `UNIQUE(project_id, action, resource)` — byte-for-byte the reference schema.
- `PermissionSaved::add` inserts one row per save-resource with `onConflictDoNothing` (mirrors
  reference `saved.ts:54-69`); `remove(id)` deletes; `list(project_id)` feeds `savedRules()`.
- Implementation is a thin store trait (`trait PermissionSaved`) with a DB-backed impl in
  oc-session over oc-database (Agent 03) and an in-memory impl for tests. Server
  `GET /api/permission/saved` and `DELETE /api/permission/saved/:id` route through it
  (`handlers/permission.rs:127-139` today return empty/204).
- V1 in-memory `approved` is process-lifetime only (reference parity — "until OpenCode is restarted").

---

## 7. Fail-closed / fail-safe behavior

- **V2 default when agent permissions are absent**: deny-all (`missingAgentPermissions`,
  `core/permission.ts:15`). A session with no agent permission config must not execute risky tools.
- **`-y/--dangerously-skip-permissions` / `--auto`**: the *only* bypass; implemented client-side as
  reply-`once` (reference `run.ts:800-804`). It must never be the default; the non-interactive
  default is auto-**reject** (`run.ts:806-814`, already in `run/events.rs`).
- **No reply / session teardown / server shutdown**: pending waiters fail (`DeclinedError`), the
  tool effect does not run, and the runner maps it to "tool execution interrupted". Never a timeout
  that silently allows.
- **Deny before side effects**: rule evaluation happens at the top of the leaf `execute`, before any
  fs/shell/network call. `CoreContext::assert` ordering in the V2 leaves already places
  `external_directory`/`edit` asserts before the write (core/write.rs:85-103, core/read.rs:94-115).
- **Prompt binding**: the ask payload is bound to `source{messageID, callID}` and the exact
  `resources`/`patterns` (from tool args at call time). A subsequent identical tool call must re-ask
  unless a rule/saved decision covers it; "always" only covers the listed patterns, not the whole
  tool (wildcard arity per reference). See TOOLS-06 for the segment-binding caveat.
- **Server reachability**: `serve` without `OPENCODE_SERVER_PASSWORD` warns (SEC-007, reference
  parity) — do not weaken; the permission gate remains the last line against a prompt-injected
  model, so it must be enforced in-process, not merely via the HTTP prompt.

---

## 8. Test list

Unit (oc-session):
1. `evaluate` precedence: later rulesets win; default `ask`; wildcard `*`/`?`/escaping; no ReDoS.
2. V2 `evaluate(action,resource)` deny/ask/allow precedence incl. saved-rules merge.
3. V2 `configured` returns deny-all when agent permission absent (fail-closed).
4. V1 `ask` deny → `DeniedError`, **no side effect** (spy that the leaf body after `ask` never ran).
5. V1 `ask` all-allow → returns without prompting (no `permission.asked` published, no pending).
6. `ask`→`reply once` resumes waiter; `reply always` adds approved rules and cascades same-session
   pendings; `reply reject` fails waiter + cascades reject to same-session pendings.
7. V2 `assert` blocks until reply; `assert` deny → `Blocked` with relevant rules.
8. Saved decisions: `add` persists per project (unique conflict ignored), `list(projectID)` scoped,
   `remove` deletes; an "always" reply with `save` auto-approves a subsequent same-pattern call
   without prompting.
9. Deferred wiring: reply to an unknown request → `NotFound` (204-error path).
10. Teardown: pending requests failed on service drop (no hung futures).

Integration (oc-session-runner + oc-server, tokio):
11. **Denial-no-side-effects**: a `deny` rule for `bash`/`write` → settle returns Declined/Blocked,
    shell never spawned, file never written (temp-dir assertions).
12. **Arg-mutation invalidation**: `write` approved once for path A; a second `write` to path B with
    the same rule shape re-asks (exact resources binding); a saved `always` on `file:*` covers B.
13. **Ask round trip**: runner suspends on `permission.asked` (SSE event observed), `reply once`
    resumes and the tool executes; `reply reject` produces a model-facing tool-error result.
14. **Cascade**: two same-session pendings; reject one → both rejected, both tool calls declined.
15. **Non-interactive default**: run without `-y` → `permission.asked` auto-rejects (matches
    reference run.ts); with `-y` → auto-approves `once`.
16. **CallID binding**: approval bound to `source.callID`; the event payload carries
    `tool{messageID,callID}`; a stale/different callID is not auto-approved.
17. **Prompt-injection / confused-deputy (server-level)**: with a deny rule, a malicious
    `permission.ask` POST cannot escalate (effect stays `deny`); a forged `permission.reply` for an
    unknown `requestID` is `NotFound`; a prompt-injected `bash` under an `ask` rule executes nothing
    until the human replies.
18. Protocol/parity (Agent 13): golden JSON for `permission.asked`/`permission.replied`,
    `PermissionCreateData{data:{id,effect}}`, `ReplyBody`, `per_` ID prefix.

---

## 9. Dependencies on other agents

| Agent | Dependency | Direction |
|---|---|---|
| 02 (INTEGRATION-001) | oc-cli wiring to server router + LocalClient so the permission HTTP surface and SSE events are reachable and testable | need (before E2E of this plan) |
| 03 (DB-001) | oc-database store wiring for `PermissionSaved` (`permission` table impl over `Database::open`) | need (persistence) |
| 07 (TOOLS-001) | ToolRegistry/ToolSettle production impl; **convert V2 `CoreTool::execute` to async** so `assert` can suspend; runner settle must call this service | need + reciprocal (this service is the settle gate) |
| 09 (TOOLS-002/3/4) | oc-tool fs/shell fixes; `ToolContext::ask`/`CoreContext::assert` hook signatures live in oc-tool — must adopt the async service handle; bounded read (TOOLS-003) is required before reads may run post-approval | need + reciprocal |
| 10 (SEC-001/003, serve) | server reachability (serve wiring), PTY ticket fix, file containment; the `session_permission_*` handlers are in oc-server (Agent 10's crate) | need + reciprocal (handlers must route into this service) |
| 13 (PROTO-001) | wire-format conformance for permission events/replies, ID prefix, SSE framing | need (golden parity) |
| 11 (async review) | validate the oneshot/deferred no-lost-wakeup design (ASYNC-001 pattern) | advisory |

Do-not-merge rule: this plan's enforcement must be merged **before** Agent 02/07's first merge that
makes a tool reachable from the binary, and before 09's read/write tools can be driven by a live
model. If the service cannot land in time, the safe fallback is to ship the runner with tools
hard-disabled (settle returns "tools disabled") so SEC-001 stays inert.

---

## 10. Merge order (security-first)

1. This plan: canonical permission service (oc-session) + oc-server handler routing + oc-tool
   async hooks — as a unit, with the fail-closed V2 default.
2. Persistence (permission table) on top (with Agent 03).
3. Agent 07 runner wiring lands **only after** the gate is active.
4. Agent 09 tool hardening (bounded reads, symlink containment) lands before wide read/write use.
5. Agent 13 protocol golden tests gate the wire surface.
6. Agent 10 serve reachability + SEC-001/003 fixes close the remote surface.

Gate check for any merge that makes tool execution reachable: `deny` rule blocks bash/write
(no side effect), `ask` rule suspends + round-trips, non-interactive default auto-rejects.
