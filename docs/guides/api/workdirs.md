# Working directories and repositories (`/api/*`)

## Overview

The read-mostly REST routes behind the new-session dialog: browsing the
filesystem for a working directory, detecting git state there, listing the
repositories and pull requests a session can be started against, and opening a
known directory in an external editor. What the dialog does with the result — a
`new_session` send carrying `workdir`, `worktree` and `provider` — is in
[sends.md](sends.md#post-apisends); conventions and error semantics are in
[README.md](README.md).

Two lists feed the dialog's tabs and are built from different sources: **Recent
directories** is derived from existing session rows (Delta keeps no separate
history), while **Repositories** aggregates the same rows by git identity and
additionally probes any registered clone roots for clones the user has never
launched a session in.

## Working directories

### `GET /api/workdir/list`

Browse one directory for the picker (read-only). Lists the immediate
subdirectories of `path` — directories only, dot-directories hidden, sorted by
name — along with the canonical path and its parent so the picker can step up.

Query parameters:

- `path` (optional) — the absolute path to list. Omitted or empty defaults to
  the user's home directory.

Response:

- **200**:

  ```json
  {
    "path": "/home/user",
    "parent": "/home",
    "entries": [{ "name": "projects", "path": "/home/user/projects" }]
  }
  ```

  `parent` is `null` at a filesystem root.

- **400** — the path does not exist or is not a directory.
- **403** — the path exists but the server cannot read it.

### `GET /api/workdir/recent`

List the directories sessions have run in, most-recently-used first. Derived
from existing session rows, so a directory disappears from this list only when
every session that used it is gone.

The list is capped at the 20 most recent directories and is not paged: an
infrequently used directory drops off the end while its sessions remain
listed by [`GET /api/sessions`](sessions.md#get-apisessions).

- **200**:

  ```json
  {
    "workdirs": [{ "path": "/work", "last_used_at": "2026-01-01T00:00:00Z" }]
  }
  ```

  `last_used_at` is the latest activity in any session that used the directory
  (ISO-8601 UTC), or `null` when unknown.

### `GET /api/workdir/git`

Detect whether a directory is inside a git repository — the gate for the
worktree-at-start option. No fetch, so it is cheap to call as the picker's
selection changes.

Query parameters:

- `path` (required) — the absolute path to inspect.

Response:

- **200**:

  ```json
  { "repo_root": "/projects/app", "default_branch": "main" }
  ```

  `repo_root` is the repository root containing `path`, and is `null` when the
  path is not inside a git repository — that is the "not a repo" answer, not an
  error. `default_branch` is the repository's default branch short name when
  known, `null` otherwise.

- **400** — `path` is missing or blank.

### `GET /api/workdir/git/branches`

List the remote branches a worktree can be based on. Resolves the repository
containing `path`, fetches the remote, and returns the branch list a start-point
picker offers.

Query parameters:

- `path` (required) — a path inside the repository.

Response:

- **200**:

  ```json
  { "default_branch": "main", "remote_branches": ["main", "feature"] }
  ```

  `remote_branches` are short names with no `origin/` prefix, excluding the
  `origin/HEAD` symref, and reflect a fresh fetch. `default_branch` is `null`
  when it is not known.

- **400** — `path` is missing or blank, or is not inside a git repository.
- **500** — the fetch itself failed (e.g. no network, or the remote refused).

### `POST /api/open-cwd`

Launch an external tool at a directory Delta already knows. Only VS Code is
registered today, spawned as `code <path>`.

Request:

```json
{ "path": "/work/delta", "handler": "vscode" }
```

- `path` (required) — the absolute path to open. It MUST be a path Delta has
  already surfaced to the browser (a `session.cwd`, `session.requested_workdir`,
  or `message.cwd`). The server checks that allowlist before invoking the
  opener, so a hand-crafted request cannot point the editor at an arbitrary
  directory on disk.
- `handler` (optional) — which tool to launch. Defaults to `vscode`, the only
  registered handler.

Response:

- **204 No Content** — the tool was spawned.
- **400** — a blank `path`; `code: "open_cwd_path_not_allowed"` when the path is
  not in the allowlist, or `code: "open_cwd_unknown_handler"` for an
  unregistered `handler` id.
- **500** — `code: "open_cwd_command_not_found"` when the tool's binary is not
  installed on the server host (the browser renders a specific "VS Code is not
  installed" message), or `code: "open_cwd_spawn_failed"` for any other spawn
  failure.

## Repositories

### `GET /api/repositories`

List the registered repositories for the dialog's Repository tab, ordered by the
most recent activity across each repository's clones.

Aggregates the session history: every distinct (repository root, clone path)
pair becomes a clone, and clones whose `origin` URL normalises to the same key
bundle under one repository. Clones whose path no longer exists on disk are
filtered out (lazy GC), and a repository drained of every clone disappears with
them. Sessions launched outside any git repository do not contribute — those
surface in [`GET /api/workdir/recent`](#get-apiworkdirrecent) instead. Each call
also probes the direct children of every registered
[clone root](#get-apiclone-roots), so a clone the user has never launched a
session in still appears.

The session-derived side is capped and not paged: only the 20 most recently
active repository roots contribute, and within each root at most 5 user-picked
clone paths plus 10 machine-generated ones (paths under the per-session
worktree base, so a burst of disposable worktrees cannot squeeze out the main
tree). A long-idle repository is therefore absent rather than at the end of the
list. Clone-root probing is not subject to those caps.

- **200**:

  ```json
  {
    "repositories": [
      {
        "identity_key": "github.com/x7c1/delta",
        "display_name": "x7c1/delta",
        "recently_used_clone_path": "/work/delta",
        "clones": [
          {
            "path": "/work/delta",
            "last_opened_at": "2026-01-01T00:00:00Z",
            "last_branch": "main",
            "last_launch_option_ids": [],
            "last_worktree_enabled": false,
            "last_worktree_start_point": null
          }
        ]
      }
    ]
  }
  ```

  - `identity_key` is the normalised `origin` URL, or the clone's absolute path
    when no origin is configured; `display_name` is the label the picker shows.
  - `recently_used_clone_path` names the clone the picker pre-selects. `clones`
    is ordered most-recent first, and `last_opened_at` is `null` when no
    contributing session has any activity yet.
  - `last_launch_option_ids`, `last_worktree_enabled` and
    `last_worktree_start_point` are the per-clone pre-fill for the launch
    controls. Per-session selections are not persisted yet, so today they are
    always `[]`, `false` and `null`.

### `POST /api/repositories/clone`

Clone a repository the user has no local clone of into one of their registered
clone roots. This is what makes a PR row whose repository exists nowhere on this
machine actionable: `gh` is already authenticated (the PR tab is gated on it) and
the clone roots already say where clones belong, so nothing else has to be asked.

Request:

```json
{
  "repo_owner": "x7c1",
  "repo_name": "delta",
  "clone_root": "/home/dev/projects"
}
```

`clone_root` must be a registered clone root, spelled exactly as
[`GET /api/clone-roots`](#get-apiclone-roots) returns it. The destination is
exactly `<clone_root>/<repo_name>` and the request never names it: there is no
fallback naming, so either that path is free or the request is refused.

- **202 Accepted** — no body. The clone runs as a background job and reports
  through the [`repository_clone_completed` /
  `repository_clone_failed`](live-channels.md#repository-clones) events on `/ws`;
  nothing about the outcome is in this response.
- **400** (body `code: "clone_root_not_registered"`) — `clone_root` is not
  registered. Register it first with `POST /api/clone-roots`, then retry with the
  path that registration's `201` returned: registration canonicalises what it is
  given (trailing slashes trimmed) while the match here is verbatim, so a retry
  that reuses the typed `/home/dev/projects/` is refused again.
- **400** (no code) — `repo_owner` or `repo_name` is not a single path component
  (blank, or carrying `/`, `\`, `..`, or a NUL).
- **409** (body `code: "clone_dest_exists"`) — `<clone_root>/<repo_name>` already
  exists. No job starts.

How a clone runs, and what that guarantees:

- The clone is assembled in a temporary sibling inside the same clone root
  (`<clone_root>/.delta-clone-tmp-<repo_name>`) and renamed onto the destination
  when it succeeds. A rename within one directory is atomic, so the destination
  is never observed half-cloned — it either does not exist or is a finished
  clone. On failure the temporary directory is removed, so a retry is simply the
  same request again.
- Jobs are tracked **in memory, keyed by destination path**. A second request for
  a destination that is already being cloned *joins* the running job: it also
  answers `202`, starts no second `gh` process, and is served by that job's one
  completion event. Requests for different repositories run concurrently.
- The registry is not persisted. A server restart forgets in-flight jobs, and the
  temporary directory such a death leaves behind is removed when the next job for
  that destination starts.
- The job is independent of every session lifecycle: starting a session while a
  clone runs neither waits for it nor delays it.

Because the job registry lives only in the server's memory, and the *intent*
behind a clone (which PR the user meant to open once it lands) lives only in the
browser's, a page reload while a clone is running loses that intent by design.
The row still reads as "no local clone" until the next refetch, and clicking it
again re-issues the request — which the dedupe turns into a join of the job that
is still running, not a second clone.

### `GET /api/clone-roots`

List the registered clone roots, newest first. A clone root is a directory where
the user's git clones live; every `GET /api/repositories` call probes its direct
children for clones.

- **200**:

  ```json
  { "clone_roots": [{ "path": "/home/dev/projects" }] }
  ```

  Only the path is on the wire; the stored `created_at` is omitted because the
  Settings list does not show it.

### `POST /api/clone-roots`

Register a clone root: a directory where the user's git clones live (not a clone
itself). Its direct children are probed for git clones on every
`GET /api/repositories`, surfacing clones the user has never launched a session
in.

Request:

```json
{ "path": "/home/dev/projects" }
```

`path` must be non-blank and absolute. Trailing slashes are trimmed for
canonicalisation, so `/home/dev/projects/` and `/home/dev/projects` register the
same row. The path is NOT required to exist or to contain git repositories at
registration time — a future-state clone root is allowed.

- **201 Created**:

  ```json
  { "path": "/home/dev/projects" }
  ```

- **400** — a blank or relative `path`.
- **409** (body `code: "clone_root_duplicate"`) — the path is already
  registered. The Settings dialog shows an inline hint instead of a failure
  toast.

### `DELETE /api/clone-roots/{path_b64}`

Unregister a clone root. The registered absolute path is URL-safe base64 in the
path segment, so its embedded `/` characters stay out of the route match. Encode
it unpadded (RFC 4648 §5): `=` is not a token character, so the padding most
base64 encoders append by default makes the segment undecodable.

- **204 No Content** — the root is gone. Deleting a path that is not registered
  is a silent no-op (idempotent), so a click never surfaces a 404 for a root
  removed in another tab.
- **400** — the segment is not a decodable URL-safe base64 token.

## Pull requests

### `GET /api/prs`

List pull requests for the dialog's PR tab, one lens per section. Queries the
GitHub search API directly via `gh api graphql`, then joins the result against
the registered repositories so each row knows whether Delta has a local clone.
`gh search prs` is not usable here: its `--json` projection cannot return the
`head_ref` and cross-fork `head_repo_owner`/`head_repo_name` fields the
composer needs.

Query parameters:

- `lens` (required) — `reviewer` (open PRs that requested your review, drafts
  excluded) or `author` (open PRs you authored, drafts included). The tab asks
  for one explicitly per section, so there is no default.

Each lens asks GitHub for at most 50 pull requests, most recently updated
first, and only those updated within the last year — an older or lower-ranked
PR is absent rather than on a later page. A lens's result is memoised for 30
seconds, so a PR opened moments ago may take one more refresh to appear.

- **200**:

  ```json
  {
    "gh_available": true,
    "pull_requests": [
      {
        "number": 42,
        "title": "feat: x",
        "repo_owner": "x7c1",
        "repo_name": "delta",
        "head_ref": "feat/x",
        "head_repo_owner": "x7c1",
        "head_repo_name": "delta",
        "draft": false,
        "url": "https://github.com/x7c1/delta/pull/42",
        "updated_at": "2026-06-24T00:00:00Z",
        "author_login": "x7c1",
        "has_local_clone": true
      }
    ]
  }
  ```

  - `gh_available` is `false` when `gh` is missing or `gh auth status` fails. In
    that case `pull_requests` is empty and the status is still **200** — the tab
    renders an inline "run `gh auth login`" hint rather than a generic failure.
    That answer is resolved once per server process and cached for its lifetime,
    so authenticating after the server started does not flip `gh_available`
    until the server is restarted.
  - `repo_owner`/`repo_name` name the base repository; `head_repo_owner`/
    `head_repo_name` differ from them for a cross-fork PR.
  - `has_local_clone` is `true` when Delta knows at least one local clone of the
    base repository. The UI de-emphasises rows where it is `false` and blocks
    the click, since there is nowhere to launch the session.

- **400** — an unknown `lens`.
