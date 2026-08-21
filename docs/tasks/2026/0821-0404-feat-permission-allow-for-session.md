---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: 'cd backend && cargo fmt --all -- --check && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cd .. && BEFORE=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && make gen && AFTER=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && [ "$BEFORE" = "$AFTER" ] && cd frontend && pnpm -r build && pnpm -r typecheck && pnpm -r test && pnpm -r lint && cd .. && make e2e'
assignee: null
branch: task/0821-0404-feat-permission-allow-for-session
created_at: 2026-08-21T04:04:25Z
updated_at: 2026-08-21T05:22:00Z
---

# feat(permission): add a session-scoped allow decision and map it to Codex `acceptForSession`

## Overview

Delta's neutral permission decision is frozen to two variants —
`PermissionDecision { Allow, Deny }`
(`backend/crates/domain/delta-usecase/src/interactor/permission_decision.rs:23-26`)
— and the Codex adapter maps them onto exactly two wire values, `accept` and
`decline` (`backend/crates/gateway/codex-agent/src/adapter.rs:185,187`, applied
in `resolve_permission` at `adapter.rs:847-893`). The adapter's own module
documentation records this as a deliberate v1 simplification:
`adapter.rs:51` says "v1 does not use the `acceptForSession` / execpolicy /
network amendment decision variants". This task lifts the `acceptForSession`
half of that restriction.

The cost of the restriction is now measurable. In a dogfooding session on
2026-08-18 a single Codex turn raised 13 file-change approvals and 3 command
approvals in sequence. Every one of them had to be answered with an individual
`Allow`, because the only decision Delta can express is "permit this one
request". The `codex app-server` protocol has carried the remedy since before
Delta's vendored pin: both approval response types accept `acceptForSession`
(`backend/crates/gateway/codex-agent/vendor/app-server-schema/CommandExecutionRequestApprovalResponse.json:27`,
`FileChangeRequestApprovalResponse.json:27`; both decision enums are documented
in `vendor/app-server-schema/README.md:73-79`). Delta simply never sends it.

The work is a contract widening that runs the full height of the stack:
neutral enum → REST wire type → generated TypeScript → store/API client → the
approval card, and on the other side neutral enum → Codex adapter → wire value.
Two constraints shape it.

**Claude must stay byte-identical.** Claude's decision leaves Delta as a hook
response whose `behavior` field is built from a boolean
(`backend/crates/apps/delta-server/src/hooks/mod.rs:181` →
`delta-wire/src/hooks/permission_request_response.rs:31-36`). There is no
session-scoped `behavior` in that hook contract, so a session-scoped decision
must never reach a Claude-backed session — and must not silently degrade into a
plain allow either, because a user who asked to stop being prompted would keep
being prompted with no explanation.

**The gate is a capability, not a provider id.** The provider capability
profile (`backend/crates/domain/delta-usecase/src/agent/capabilities.rs:148-162`)
already exists precisely so behaviour like this is declared rather than
branched on a name, and `WireProviderCapabilities`
(`backend/crates/gateway/delta-wire/src/rest/providers_response.rs:22-51`)
documents itself at `:12-19` as the place "a further UI-relevant capability can
join … without reshaping the response". Follow that seam. `#302` and `#317`
established the pattern; do not introduce a `provider === 'codex'` test
anywhere, in Rust or in TypeScript.

### Required shape of the change

1. **Neutral enum.** Add a third variant to `PermissionDecision`
   (`permission_decision.rs:23-26`) for the session-scoped allow. It is
   `Copy`/`PartialEq` and is compared with `==` against `Allow` in three
   places — `permission_decision.rs:71`, `permission_decision.rs:149`, and
   `hooks/mod.rs:181`. Every one of those comparisons currently means "is this
   an allow?", so revisit each rather than letting the new variant fall through
   to the `false` branch.

2. **Capability.** Add a dedicated field to `AgentCapabilities`
   (`capabilities.rs:148-162`) declaring whether the provider accepts a
   session-scoped allow. Do **not** fold it into `PermissionCapability`
   (`capabilities.rs:74-82`): that enum answers *where* a decision is made
   (adapter / hook / provider-owned), which is a different question from *which
   decisions exist*. Follow the file's established style — every field on the
   profile is an enum, not a bool. Declare it explicitly for both providers:
   Claude at `claude-agent/src/lib.rs:77-89` (not supported), Codex at
   `codex-agent/src/adapter.rs:144-157` (supported). The composition root
   fan-out is `delta-bootstrap/src/lib.rs:67-72`.

3. **Wire projection to the browser.** Extend `WireProviderCapabilities`
   (`providers_response.rs:22-51`) with the derived flag, deriving it in the
   existing `From<AgentCapabilities>` at `:74-82`. The provider-capability JSON
   is asserted in `delta-server/src/app.rs:479-507`; extend those assertions for
   both providers.

4. **REST decision body.** Extend `WirePermissionDecision`
   (`delta-wire/src/rest/permission_decision_request.rs:8-33`) and its
   `From<WirePermissionDecision> for PermissionDecision` at `:16-23`. The enum
   is `snake_case` with `ts(rename = "PermissionDecision")`, so the generated
   `frontend/packages/gateway/wire-gen/src/generated/PermissionDecision.ts:6`
   union gains the new member. Regenerate with `make gen` — the check command
   fails if generated output is not committed.

5. **Reject the unsupported combination explicitly.** A session-scoped decision
   posted against a provider that does not declare the capability must fail with
   a documented `400` and a stable error code — not a `500`, and not a silent
   downgrade to a plain allow. Add the `Error` variant next to
   `Error::PermissionNotPending` (`delta-usecase/src/error.rs:44-46`) and map it
   in `delta-server/src/api/api_error.rs` alongside the existing
   `permission_not_pending` code (`api_error.rs:17,125-126`). This is the
   backstop that keeps the Claude hook path
   (`hooks/mod.rs:181`) unreachable for the new variant even if a client sends
   it directly.

6. **Persistence stays boolean — deliberately, and say so.** The store records
   the decision as a bool (`store.decide_permission_request(request_id, allowed)`,
   called from `permission_decision.rs:71` and `:149`). A session-scoped allow
   *is* an allow for the purposes of that row: the row records whether this tool
   call was permitted, while the session-scope is a provider-side side effect
   Delta neither owns nor replays. Map it to `true` and leave the schema alone —
   no `SCHEMA_VERSION` bump. Put the reasoning in a code comment at the mapping
   site so the next reader does not mistake it for a lost distinction.

7. **Codex adapter.** Add the `acceptForSession` wire constant next to
   `DECISION_ACCEPT` / `DECISION_DECLINE` (`adapter.rs:185-187`) and extend the
   mapping in `resolve_permission` (`adapter.rs:870-873`). Both approval kinds
   share the one `{ "decision": … }` reply path (`adapter.rs:876-886`), so a
   single mapping serves command-execution and file-change alike. Update the
   module documentation at `adapter.rs:51`, which currently states the opposite,
   and the vendored schema README at
   `vendor/app-server-schema/README.md:73-79` if it claims the variant is unused.

8. **Frontend.** `PermissionNotice`
   (`frontend/packages/apps/web/src/features/transcript/PermissionNotice.tsx`)
   renders Allow / Deny / Dismiss at `:191-201` via `decide()` at `:127-140`.
   Add the session-scoped action as a third affirmative button, rendered only
   when the focused provider declares the capability. Plumb the flag the same
   way `providerHasTerminal` is plumbed today — `WorkspaceScreen.tsx:157-161`
   builds `capabilitiesByProvider`, `:392-403` resolves the focused entry, and
   `:529` passes the raw tri-state value down through
   `TranscriptPane.tsx:157-164,183,886`.

   **The unknown-capability default is `false`, opposite to `has_terminal`.**
   `has_terminal` defaults to `true` when unknown
   (`PermissionNotice.tsx:72`, `HAS_TERMINAL_WHEN_UNKNOWN`) because its fallback
   text is merely advice. This flag gates a button that performs an action, and
   a button that fails when pressed is worse than an absent one — hide it unless
   the capability is known to be present. Give the new constant the same kind of
   named, commented definition so the asymmetry is visible rather than
   accidental.

9. **The fake provider must learn the new value.** `fake-codex`'s approval step
   echoes the decision it received (`fake-codex/src/scenario.rs:50,89-90,315`),
   and `comms_log.rs:218,324,341` asserts the logged payload's
   `["result"]["decision"]`. Teach the fake the new value so the full-loop tests
   below can assert it end to end.

10. **Docs.** The repository runs coverage gates that fail the build when the
    API surface drifts from `docs/guides/api/` (route coverage from `#312`,
    `/ws` event coverage from `#313`). Update the decision shape and the new
    error code where they are documented — `docs/guides/api/shapes.md`,
    `sessions.md`, `sends.md`, `live-channels.md` — following each file's
    existing structure. Keep the provider write-up capability-phrased, not
    Codex-phrased.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A `fake-codex` full-loop test (alongside the existing four at
      `backend/crates/apps/fake-codex/tests/full_loop.rs:1606-1650`) drives a
      session-scoped allow for a `commandExecution` approval and asserts the
      exact wire value `acceptForSession` reaches the provider.
- [x] The same is asserted for a `fileChange` approval, proving the shared
      reply path serves both approval kinds.
- [x] A test asserts that posting the session-scoped decision to a session whose
      provider does not declare the capability is rejected with the documented
      `400` error code, and that no decision reaches the provider and the
      permission row stays pending.
- [x] A Claude-path test asserts the hook response is unchanged for `Allow` and
      `Deny` — the existing hook `behavior` payloads remain byte-identical.
- [x] `delta-server/src/app.rs` provider-capability assertions cover the new
      flag for both providers, with Claude declaring it unsupported and Codex
      supported.
- [x] `delta-wire` round-trip tests cover the new `snake_case` decision value
      (extending `permission_decision_request.rs:36-47`), and `make gen` output
      is committed (enforced by the check command's hash comparison).
- [x] `PermissionNotice.test.tsx` covers three cases: the button is rendered
      when the capability is present, absent when it is declared unsupported,
      and absent when the capability is unknown.
- [x] An `e2e-fake` spec exercises the session-scoped allow through the browser
      to the fake provider, alongside the existing
      `frontend/packages/apps/web/e2e-fake/permission-decision.spec.ts`.
- [x] `grep` finds no provider-id literal (`"codex"` / `'codex'`) introduced by
      this change in the permission decision path, in either Rust or TypeScript.

### Manual / on-hardware (verified by a human before merge)

- [ ] In a real `codex app-server` session, answering a command approval with
      the session-scoped allow suppresses subsequent approval prompts for
      comparable commands in the same session, and the turn completes.
- [ ] The same is verified for a file-change approval.
- [ ] A real Claude session shows no session-scoped button and its approval
      prompts behave exactly as before.

## Out of scope

- **Enriching the approval card's contents.** Correlating
  `item/fileChange/requestApproval.params.itemId` with the `item/started`
  `FileChangeThreadItem` to show target paths, change kinds and diffs — instead
  of today's 120-character JSON summary (`PermissionNotice.tsx:9,16-34`) — is a
  separate task with a separate failure surface. It does not depend on this one.
- **The remaining Codex decision variants.** `acceptWithExecpolicyAmendment`,
  `applyNetworkPolicyAmendment` and `cancel` stay unmapped; each needs a neutral
  projection that does not exist yet.
- **`item/permissions/requestApproval`.** It answers with a
  `GrantedPermissionProfile`, not a decision, and deliberately takes the
  `UnsupportedInteraction` path (`adapter.rs:1050-1071`). Leave it there.
- **Re-vendoring the app-server schema.** The vendored pin is `0.144.4`
  (`codex-agent/src/schema.rs:14`) while a newer Codex CLI is installed, so the
  local-only, `#[ignore]`-marked canary
  `vendored_schema_matches_the_real_generator`
  (`codex-agent/tests/real_codex_canary.rs`) fails on the version check before
  it compares anything. That failure predates this task and does not block it:
  both approval response schemas and both decision enums were diffed against the
  newer generator output and are unchanged. Do not re-vendor here.
- **Persisting session-scoped grants on the Delta side.** Delta does not track
  which grants a provider is holding; the scope lives entirely in the provider's
  session.
