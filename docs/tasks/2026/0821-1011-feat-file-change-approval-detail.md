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
branch: task/0821-1011-feat-file-change-approval-detail
created_at: 2026-08-21T10:11:44Z
updated_at: 2026-08-21T12:40:00Z
---

# feat(permission): show the affected paths and diff on a file-change approval

## Overview

A Codex file-change approval card currently tells the user almost nothing about
what they are approving. `PermissionNotice`
(`frontend/packages/apps/web/src/features/transcript/PermissionNotice.tsx:16-34`)
builds its one-line summary by looking for `command`, `file_path`, `path`, or
`url` in the request's input JSON, and falling back to the raw JSON truncated at
120 characters. A **command** approval carries `command` in its params, so it has
shown the real command since the browser first answered permissions. A
**file-change** approval carries none of those keys, so it falls all the way
through to the truncated-JSON branch.

That is not a formatting problem — the information genuinely is not there. The
approval request's params are sparse by design (vendored schema,
`backend/crates/gateway/codex-agent/vendor/app-server-schema/FileChangeRequestApprovalParams.json`):

    { itemId, startedAtMs, threadId, turnId, grantRoot?, reason? }

No path, no kind, no diff. But the same information is already on the wire a
moment earlier. The `item/started` notification for the same item carries a
`FileChangeThreadItem`:

    { id, status, changes: [ { path, kind, diff } ] }

and `requestApproval.params.itemId` equals that `item.id`. Delta throws the
useful half away because nothing correlates the two.

The fix is to correlate them in the Codex adapter and put the result on the
neutral permission event, so the card can show which files are affected, how
each one changes, the provider's stated `reason`, and — on expand — the diff.

This matters in practice: a dogfooding session on 2026-08-18 raised 13
file-change approvals in a single turn, each rendered as an interchangeable
120-character JSON blob. The session-scoped allow shipped separately and reduced
the *number* of prompts; this task makes the remaining prompts answerable.

**A second half surfaced from live use of the first.** Codex does not always
edit through the structured patch tool. In a real session it created a file that
way — raising a proper `fileChange` approval with the detail this task adds — and
then probed for the `apply_patch` executable on `PATH` and shelled out to it for
every subsequent edit:

    zsh -lc "printf '%s\n' '*** Begin Patch' '*** Update File: notes.txt' '@@' \
      '-before' '+after' '*** End Patch' | .../apply_patch"

Those arrive as **command-execution** approvals, so no `item/started` file-change
item exists to correlate and there is nothing for the detail path to attach. The
patch is nevertheless right there in the command string — and invisible, because
the card truncates its summary at 120 characters, which in that example cuts off
exactly where the patch body begins. The user is asked to approve an edit whose
content is on screen but clipped.

So the same failure this task exists to fix reappears one branch over, for the
same reason: a one-line summary is the only rendering a permission ever gets. The
fix is symmetric — give the summary the expand affordance the diff already has,
so any request whose summary had to be truncated can be read in full.

### Required shape of the change

1. **Correlate in the adapter, not the UI.** The comms log is explicitly a
   non-durable, lossy observability channel — do **not** build this by having
   the browser re-read `/comms`. The adapter owns the correlation and puts the
   display information on the neutral permission event.

   The adapter's `translate_loop`
   (`backend/crates/gateway/codex-agent/src/adapter.rs`, the `ServerRequest`
   arm) is the only stateful seam available on the receive path:
   `translate_notification` and `classify_server_request`
   (`codex-agent/src/translate.rs:172`, `:414`) are pure functions by design.
   `CodexSession` (`adapter.rs:203-233`) already holds per-session state
   (`approvals`, `current_turn_id`); the item map belongs alongside them.

   The alternative — enriching later in the pump from a runtime map fed by
   `AgentEvent::ToolStarted` — is **not** the chosen design: it would spread one
   provider's wire quirk across the neutral core.

2. **Track file-change items from `item/started`.** `item_event`
   (`translate.rs:485-513`) routes a `fileChange` item
   (`FILE_CHANGE_ITEM_TYPE`, `translate.rs:92`) to `tool_event`
   (`translate.rs:613-635`), which already carries the whole item in
   `input_json`. Keep an `itemId -> changes` map in the session so the approval
   request can look its item up.

3. **Refresh on `FileChangePatchUpdatedNotification`.** The protocol emits
   `{ itemId, threadId, turnId, changes: FileUpdateChange[] }` to revise an
   item's changes after `item/started`. A map populated once at `item/started`
   and never updated will show a stale diff. Handle this notification and
   replace the entry's changes.

4. **Put the detail on the neutral event.** `AgentPermissionRequest`
   (`backend/crates/domain/delta-usecase/src/agent/event.rs:72-81`) carries
   `request_id`, `tool_name`, `input_json`, `tool_use_id`. Extend it so a
   file-change approval arrives with its affected paths, each change's kind, and
   each change's diff, plus the provider's `reason` when present.

   Keep the shape provider-neutral — this is "the files this request would
   change", not "Codex's `FileUpdateChange` array". Claude's permission events
   must serialize byte-identically to today.

5. **Fall back explicitly when correlation fails.** If no item is known for the
   `itemId` — the notification was missed, the item arrived out of order, the
   session was resumed mid-turn — the card must fall back to today's JSON
   summary rather than rendering an empty or misleading detail block. Make the
   fallback a deliberate, tested branch, not an accident of a `None` flowing
   through.

6. **Clean the correlation state up.** An unbounded per-session map is a leak.
   Drop an item's entry when the item completes (`item/completed`), at turn end,
   and on connection loss — the same lifecycle points the existing `approvals`
   map and `CommsLogHub` already observe. A session that runs for hours must not
   accumulate every file-change item it ever saw.

   *(This item originally also demanded a drop at approval resolution.
   Implementation showed that costs a structural change — `resolve_permission`
   is keyed by the neutral request id and holds no item id — while buying
   nothing: a declined item still retires via `item/completed`, and turn end
   clears the map unconditionally, so it is bounded by the unfinished
   file-change items of a single turn either way. The deviation is deliberate
   and the reasoning is recorded on the map's own module doc.)*

7. **Wire and frontend.** Carry the detail through the wire permission shapes
   (`delta-wire`'s `session_event.rs` `PermissionRequested` and
   `rest/sends_response.rs`'s `WirePendingPermission`, which re-seeds the card
   after a reconnect — both surfaces must agree), regenerate the TypeScript with
   `make gen`, and render it in `PermissionNotice.tsx`: the affected paths and
   their change kinds always visible, the `reason` when present, and the diff
   behind an expand control rather than inline.

   Follow the file's existing patterns for the expand control and keep the card
   inside the transcript flow — `#319` deliberately moved it there.

8. **Do not branch on the provider id.** In Rust and in TypeScript alike. A
   Claude permission simply has no file-change detail and renders exactly as it
   does today; that is a property of the data, not a `provider === 'codex'`
   test.

9. **Let a truncated summary be expanded to its full text.** `toolInputSummary`
   (`PermissionNotice.tsx`) clips at `SUMMARY_MAX_CHARS = 120`. When it clips,
   the card must offer the untruncated text behind the same `Collapsible` the
   diff uses — one affordance, one mental model. When it does not clip, add no
   control: a short command must look exactly as it does today.

   This is **frontend-only**. The full text already reaches the browser in
   `tool_input`; nothing on the wire or in either adapter changes.

   **Do not special-case `apply_patch`, and do not parse the command string.**
   Detecting a patch in a command and rendering it as a diff would be Codex-
   specific, string-match dependent, and would break the moment an edit arrives
   via `sed` or a heredoc. The generic rule — long summaries can be opened —
   serves every provider and every long command, including the JSON fallback a
   file-change approval takes when correlation fails.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A `fake-codex` full-loop test drives `item/started` for a `fileChange`
      item followed by its approval request, and asserts the neutral permission
      event carries the item's paths, change kinds, and diffs.
- [x] A test with two `fileChange` items in flight concurrently asserts each
      approval gets its own item's detail — the correlation never crosses them.
- [x] A test asserts `FileChangePatchUpdatedNotification` replaces an item's
      changes, so the approval raised afterwards shows the revised diff and not
      the one from `item/started`.
- [x] A test asserts an approval whose `itemId` has no known item falls back to
      the existing JSON summary and renders no detail block.
- [x] A test asserts the correlation map is emptied at turn end, on item
      completion, and on connection loss, so a long session does not accumulate
      entries.
- [x] A Claude-path test asserts the serialized permission event and the sends
      envelope's pending-permission shape are byte-identical to before.
- [x] `PermissionNotice.test.tsx` covers: paths and kinds rendered, `reason`
      rendered when present and absent when not, diff hidden until expanded, and
      the no-detail fallback rendering today's summary.
- [x] An `e2e-fake` spec approves a file change and asserts the browser shows
      the affected path rather than a JSON blob.
- [x] `grep` finds no provider-id literal (`"codex"` / `'codex'`) introduced by
      this change in the permission path, in either Rust or TypeScript.
- [x] A test asserts a command approval whose command exceeds the summary limit
      renders the clipped line plus an expand control, and the full command text
      once expanded.
- [x] A test asserts a command approval short enough not to be clipped renders no
      expand control at all.
- [x] A test asserts a file-change approval that fell back to the JSON summary
      gets the same expand treatment when that summary was clipped.

### Manual / on-hardware (verified by a human before merge)

- [ ] In a real `codex app-server` session, a file-change approval card names
      the file(s) it would change and shows the diff on expand.
- [ ] A real Claude session's approval cards are unchanged.

## Out of scope

- **The remaining Codex decision variants.** `acceptWithExecpolicyAmendment`,
  `applyNetworkPolicyAmendment` and `cancel` stay unmapped.
- **Command-execution approval cards.** They already show the command
  (`toolInputSummary` finds `command` in the params) and this task does not
  restyle them.
- **Re-vendoring the app-server schema.** The pin is `0.144.4`
  (`codex-agent/src/schema.rs:14`) while a newer Codex CLI is installed, so the
  local-only, `#[ignore]`-marked canary `vendored_schema_matches_the_real_generator`
  fails on the version check before comparing anything. That predates this task
  and does not block it: `FileChangeThreadItem`, `FileUpdateChange` and
  `FileChangeRequestApprovalParams` were each diffed against the newer
  generator's output and are unchanged. Do not re-vendor here.
- **Applying the same treatment to `item/permissions/requestApproval`.** It
  answers with a `GrantedPermissionProfile`, not a decision, and keeps its
  `UnsupportedInteraction` path.
- **Persisting the detail.** It is display information for a live prompt; a
  resolved permission row does not need to keep it.
