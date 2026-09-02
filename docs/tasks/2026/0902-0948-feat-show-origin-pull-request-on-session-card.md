---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && ! grep -q "session-last-activity" frontend/packages/apps/web/src/features/navigator/SessionNode.tsx && grep -q "session-pull-request" frontend/packages/apps/web/src/features/navigator/SessionNode.tsx && grep -q "pull_request_number" backend/crates/gateway/delta-sqlite/src/migrations/session.rs && grep -q "pull_request_number" frontend/packages/apps/web/e2e/pr-tab.spec.ts && ! grep -q "session-last-activity" frontend/packages/apps/web/e2e/provider-marker.spec.ts'
assignee: null
branch: task/0902-0948-feat-show-origin-pull-request-on-session-card
created_at: 2026-09-02T09:48:21Z
updated_at: 2026-09-02T11:19:35Z
---

# feat(navigator): show the originating pull request on the session card instead of last activity

## Overview

Each session card in the navigator (left pane) renders, on its second line,
the launch-time repository label on the left and the session's
`last_activity_at` on the right, formatted as `YYYY-MM-DD HH:mm`
(`frontend/packages/apps/web/src/features/navigator/SessionNode.tsx:198`,
`:399–408`, `data-testid="session-last-activity"`). Dogfooding showed that
timestamp is almost never what the user needs from the list: sessions are
already ordered by recency, and the question the card should answer is
"which pull request is this session working on?". Most sessions are started
from the New session screen's **PR tab**, and today that provenance is lost
the moment Send is pressed.

Replace the timestamp with the originating pull request: the card shows
`#<number>` (e.g. `#138`) in the same right-hand slot, and clicking it opens
the PR on GitHub in a new tab. A session that was not started from a PR
pick shows nothing in that slot — no timestamp, no placeholder.

### Where the PR number is today, and where it has to go

- The PR tab's pick (`PRTab.tsx:152` `onPickPr`, and the clone-completion
  path at `:147`) calls `composerStore.setNewSessionWorkdirFromPr`
  (`store/composerStore.ts:334`), which records
  `newSessionWorkdirSource = { kind: 'pr', number, url, repo_owner,
  repo_name, head_ref }` and locks the worktree section to the PR's head
  branch (`WorktreeOptions.tsx:102` renders the lock; the toggle and
  start-point selector are not rendered, so the user cannot detach the
  compose from the PR without re-picking a directory, which resets the
  source to `{ kind: 'directory' }` at `composerStore.ts:322`). So at Send
  time, `source.kind === 'pr'` is exactly "this session is being opened
  from a PR".
- That is as far as it goes. `Composer.tsx:173` assembles the
  `NewSessionLaunch` (`store/live/sendsSlice.ts:19`: `workdir`,
  `launchOptionIds`, `provider`, `worktree`) without the PR; the request
  body (`features/composer/newSessionRequest.ts` `newSessionSendBody`)
  carries no PR field; the wire `WireCreateSendRequest`
  (`backend/crates/gateway/delta-wire/src/rest/send_request.rs:32–87`) has
  none; `SendTarget::NewSession`
  (`backend/crates/domain/delta-usecase/src/send_target.rs:48`) has none;
  and the `session` table (`delta-sqlite/src/migrations/session.rs`) has
  no column for it.

### Design

**Store the PR number only, and build the URL from it.** Delta's PR
listing is `gh`-driven and GitHub-only (`list_pull_requests.rs:16` fixes
`GITHUB_HOST = "github.com"`; local clones are matched to a PR by a
`github.com/<owner>/<name>` identity key derived from the clone's `origin`),
and the session already stores the spawn-time `repository_display_name`
(`org/repo`, derived from the same `origin`, `delta-model/src/session.rs:89`).
For a PR-picked session the two therefore name the same repository, so the
PR's web URL is `https://github.com/<repository_display_name>/pull/<number>`
— no URL column, no owner/name columns.

1. **Schema** — add a nullable `pull_request_number INTEGER` column to
   `session` as a new additive step in
   `backend/crates/gateway/delta-sqlite/src/migrations/session.rs`
   (`Step::additive(7, "ALTER TABLE session ADD COLUMN pull_request_number INTEGER;")`
   — the ladder currently tops out at `user_version` 6 and must stay
   gapless). Document the column in the module's header comment next to
   `branch_at_launch` / `repository_display_name`: a spawn-time snapshot of
   the PR the session was opened from; NULL for a session started from the
   Repository/Directory tab, for a session created by a hook-registered
   `claude` that Delta did not spawn (`hooks/register_session_row.rs:62`),
   and for every row that predates the column. There is no backfill.
2. **Domain + store** — thread the number through the same seams
   `repository_display_name` uses: `delta_model::Session`
   (`pull_request_number: Option<i64>`), the `NewSession` port
   (`ports/new_session.rs`), `SessionStore::insert_spawning_session`
   (`ports/session_store.rs:129`, the sqlite impl at
   `delta-sqlite/src/store/session_store.rs:41` and
   `store/sessions.rs:149–192`, `SESSION_COLS` / the row mapper at
   `store/sessions.rs:28–63,100`), the fake store
   (`interactor/testing/fake_store.rs`), `delta-bootstrap/src/lib.rs:534`,
   and the wire twin `delta-wire/src/session.rs:108,134`. The number must
   be written on the **spawning insert** (`spawn_fresh.rs:297` for Claude,
   `adapter_session/spawn_adapter_session.rs:175` for Codex, via
   `SendTarget::NewSession`), not on registration: the list row exists from
   the moment the send is accepted, so a `Starting` card already shows its
   PR. It is a snapshot — never updated on resume.
3. **Request** — `WireCreateSendRequest` gains
   `pull_request_number: Option<i64>` (serde default; only meaningful with
   `new_session: true`, ignored on a thread send exactly like `workdir`).
   `into_target` copies it onto `SendTarget::NewSession`. A non-positive
   number is a shape error → `400` via a new `SendTargetError` variant with
   a message in the style of the existing ones. Regenerate the TS bindings
   (`make gen`) so `SendRequest.ts` / `Session.ts` under
   `frontend/packages/gateway/wire-gen/src/generated/` carry the field.
4. **Frontend launch** — `NewSessionLaunch` gains
   `pullRequestNumber: number | null`; `Composer.tsx:173` fills it from
   `newSessionWorkdirSource` (`source.kind === 'pr' ? source.number : null`);
   `newSessionSendBody` emits `pull_request_number` only when non-null (the
   same omit-when-absent rule as `workdir` / `worktree`). Because
   `SpawnItem extends NewSessionLaunch` (`store/live/spawnsSlice.ts:24`)
   and the failed-launch **Retry** re-sends the retained launch, the number
   survives a retry with no extra plumbing — keep it that way and cover it
   with the existing retry test pattern.
5. **Card** — in `SessionNode.tsx` delete the `lastActivity` computation,
   the `formatLocalDateTime` import (the util stays; `MessageItem.tsx` uses
   it) and the `session-last-activity` span; update the component's doc
   comment and the line-2 comment, which both describe "the last-activity
   time on the right". Render `#<number>` in the freed right-hand slot with
   `data-testid="session-pull-request"` as an `<a href=… target="_blank"
   rel="noopener noreferrer">` whose accessible name / `title` names the
   PR (e.g. `Open pull request #138 on GitHub`). The link **must not be
   nested inside the card's focus `<button>`** (interactive content inside
   a button is invalid HTML and the click would also focus the session):
   restructure the header so the link is a sibling of the button in the
   header flex row (next to the kebab `Menu`), visually aligned with line 2
   — a click opens the PR and does nothing else. Keep the `memo` contract
   documented on the component (only primitives / stable values as props).
   Build the URL in one small pure helper with its own unit test (a natural
   home is next to `displayBranch` in `@delta/model`, or under
   `apps/web/src/utils/`): given `repository_display_name` and the number it
   returns `https://github.com/<org>/<repo>/pull/<n>`; when the display
   name is `null` or is not of the `<org>/<repo>` shape (the backend falls
   back to a working-tree basename when the clone has no `origin`) the
   helper returns `null` and the card renders `#<number>` as plain text
   rather than a wrong link.
6. **Tests and fixtures to update** — `SessionNode.test.tsx:318` and
   `WorkspaceScreen.test.tsx:829–852` assert the formatted timestamp;
   `e2e/provider-marker.spec.ts:78` uses `session-last-activity` as the
   meta-line colour sample (switch it to `session-repo`);
   `e2e/pr-tab.spec.ts:54,102` `toMatchObject` the send body (add
   `pull_request_number` with the picked fixture's number — the reviewer
   fixture rows are at `testing/api-mocks/src/fixtures.ts:1007,1021,1042`);
   `testing/api-mocks/src/handlers.ts:552–560` builds the mocked session
   for a `new_session` send (carry the request's number onto the mocked
   `session`, so the e2e list shows it) and `fixtures.ts` session fixtures
   need the new field; `delta-wire` JSON fixtures at
   `session.rs:160–196` and `rest/sessions_response.rs:64,90`;
   `delta-server/tests/end_to_end.rs` already reads `sessions[0]` fields.

### Session-state coverage

The new element is a passive read of a spawn-time snapshot plus an
external link; it never sends anything to the session. Per the state
matrix in the task-authoring conventions, the Automated criteria cover the
list row in every state it can be in: **spawning** (row inserted at
accept — the number is already there), **open + idle**, **open +
mid-turn**, **closed** (and closed → resuming, which reads the same stored
row). The rendering is identical in all four; what the criteria pin is
that the number is set at the spawning insert and survives close/resume
untouched.

### Pipeline notes

- Run `cargo fmt` and `make lint` before finishing the work phase.
- This task changes wire types, so `make gen` must be run and the
  regenerated files under `frontend/packages/gateway/wire-gen/src/generated/`
  are part of the change. `make gen-check` diffs that directory against
  `HEAD`, so a pre-commit `make check` fails on the regenerated files
  alone: the check phase runs `make check` on a temporary WIP commit and
  drops it (`git reset --soft origin/main`) once green, before the normal
  finalize.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `session` has a nullable `pull_request_number` column added as a new
      additive migration step (`user_version` 7) in
      `delta-sqlite/src/migrations/session.rs` (grep gate in
      `check_command`), and a sqlite store test shows a row inserted before
      the step reads back `pull_request_number: None` after migration.
- [x] `SessionStore::insert_spawning_session` persists the number and the
      session list / `get` round-trip it (sqlite store test + fake store).
- [x] A `POST /api/sends` with `new_session: true` and
      `pull_request_number: 138` produces a session whose `GET /api/sessions`
      row carries `"pull_request_number": 138` **while it is still
      `spawning`** and still after it registers; a `new_session` send
      without the field yields `null` (`delta-server/tests/end_to_end.rs`
      or the usecase spawn tests, for both the Claude and the Codex spawn
      paths).
- [x] A `pull_request_number` on a thread send is ignored, and a
      non-positive value on a `new_session` send is rejected with `400`
      (`send_request.rs` unit tests: it rides on the `NewSession` target
      and is absent → `None`).
- [x] The number is unchanged across close and resume of the session
      (usecase or store test: `close` + `ensure_open`/resume leave the
      stored value intact).
- [x] `newSessionSendBody` emits `pull_request_number` only when
      `pullRequestNumber` is non-null; `Composer` fills it from a `pr`
      workdir source and sends `null` after a directory pick; the failed
      launch Retry re-sends the same number (vitest, extending the existing
      `newSessionRequest` / `Composer` / retry tests).
- [x] `SessionNode` renders `session-pull-request` as an anchor with
      `href="https://github.com/x7c1/delta/pull/138"` and `target="_blank"`
      for a session with `repository_display_name: "x7c1/delta"` and
      `pull_request_number: 138`; renders it as plain text `#138` when the
      display name is `null` or not `<org>/<repo>`-shaped; renders no
      element in the slot when `pull_request_number` is `null`; and the
      anchor is not a descendant of the `session-node` button (vitest).
- [x] The URL helper has unit tests for the `org/repo` case, the `null`
      display name, and a basename-shaped display name.
- [x] `SessionNode.tsx` no longer references `session-last-activity` or
      `formatLocalDateTime` (grep gate in `check_command`), and no
      remaining unit or e2e test queries `session-last-activity`
      (`provider-marker.spec.ts` grep gate in `check_command`).
- [x] `e2e/pr-tab.spec.ts` asserts the send body carries
      `pull_request_number` equal to the picked fixture PR's number (grep
      gate in `check_command`), and the resulting session card shows that
      number.

### Manual / on-hardware (verified by a human before merge)

- [x] On a real Delta run, start a session from the PR tab and confirm the
      card shows `#<number>` from the moment it appears (still `Starting`),
      that clicking it opens the PR in a new browser tab without changing
      the focused session, and that a session started from the Directory
      tab shows nothing in that slot.
- [x] Pre-existing sessions (rows written before this change) render with
      an empty right-hand slot and the list still orders by recency.

## Out of scope

- Backfilling `pull_request_number` for sessions that predate the column.
- Non-GitHub hosts (GitHub Enterprise): Delta's PR flow is `github.com`-only
  today; the helper simply cannot form a URL for anything else.
- Showing the PR title, state, or CI status on the card.
- Attaching a PR to a session started from the Repository or Directory
  tab, or editing the PR of an existing session.
- Removing `last_activity_at` from the wire or the database — it still
  drives the list ordering and the Recent dirs query.
