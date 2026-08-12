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
branch: task/0812-1220-docs-api-route-coverage
created_at: 2026-08-12T12:20:00Z
updated_at: 2026-08-12T15:18:00Z
---

# docs(api): document every declared route and gate prose coverage on ENDPOINTS

## Overview

`delta_wire::endpoint::ENDPOINTS`
(`backend/crates/gateway/delta-wire/src/endpoint/table.rs`) is now the
machine-readable inventory of the API surface: 43 routes, mounted through a
binder that refuses to build a router that disagrees with the table. The prose
API docs (`docs/guides/api/`) lag far behind it: `rest.md` documents 8 of the
30 `/api/*` routes and `hooks.md` documents 3 of the 9 hooks, and
`docs/guides/api/README.md` currently has to warn readers that "a route
missing here is a gap in the prose". This task closes that gap and then locks
it shut.

Two parts:

1. **Document every declared route.** For each route in `ENDPOINTS` that the
   prose does not yet describe, add a section in the same style as the
   existing ones (purpose paragraph, request/response JSON where applicable,
   status-code bullets). Write from the source of truth: the handler doc
   comments in `backend/crates/apps/delta-server/src/api/mod.rs` and
   `backend/crates/apps/delta-server/src/hooks/`, and the wire types in
   `backend/crates/gateway/delta-wire/src/rest/` and `.../src/hooks/` (field
   docs there are rich). Do not invent semantics: if a behavior is not stated
   in code or its doc comments, verify it by reading the handler body; if
   still ambiguous, describe only what is certain.

   Missing REST routes (add to the REST docs): `POST
   /api/sessions/{id}/interrupt`, `GET /api/sessions/{id}/sends`, `POST
   /api/sends/{id}/cancel`, `POST /api/sends/{id}/release`, `POST
   /api/permissions/{id}/decision`, `POST
   /api/sessions/{id}/questions/{request_id}/answer`, `POST
   /api/sessions/{id}/questions/cancel`, `GET /api/workdir/list`, `GET
   /api/workdir/recent`, `GET /api/workdir/git`, `GET
   /api/workdir/git/branches`, `POST /api/open-cwd`, `GET /api/repositories`,
   `GET /api/repository-scan-roots`, `POST /api/repository-scan-roots`,
   `DELETE /api/repository-scan-roots/{path_b64}`, `GET /api/prs`, `GET
   /api/launch-options`, `POST /api/launch-options`, `PATCH
   /api/launch-options/{id}`, `DELETE /api/launch-options/{id}`, `GET
   /api/version`.

   Missing hooks (add to `hooks.md`): `POST /hooks/message-display`, `POST
   /hooks/post-tool-use`, `POST /hooks/permission-request`, `POST
   /hooks/session-start`, `POST /hooks/session-end`, `POST
   /hooks/status-line`.

   `rest.md` (247 lines) would more than double; per this repo's docs policy
   (split large documents by topic from the start), split the REST reference
   into per-area files inside `docs/guides/api/` (e.g. sessions, sends and
   permissions/questions, workdir and repositories, launch options and misc —
   choose groupings that keep each file focused), update the map in
   `docs/guides/api/README.md`, and update any inbound links to `rest.md`
   anchors (search the repo, including `docs/guides/api/*.md` cross-links).

2. **Gate the coverage.** Add a test in `delta-wire` that iterates the real
   `ENDPOINTS` table and asserts each entry's route label (the
   `route_label()` form, e.g. `POST /api/sends`) appears in at least one
   markdown file under `docs/guides/api/` (resolve the docs directory from
   `CARGO_MANIFEST_DIR`). Because it walks the real table, a future PR that
   declares a new endpoint without documenting it fails `cargo test` — the
   prose can no longer silently drift. Keep the assertion message actionable
   (name the undocumented route and the docs directory).

   With the gap closed, soften the "does not describe every route yet"
   caveat in `docs/guides/api/README.md` to state the new invariant: every
   declared route is documented, and the coverage test keeps it that way.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A new `delta-wire` test iterates `ENDPOINTS` and asserts every route
      label appears in the markdown under `docs/guides/api/`; it runs as part
      of `cargo test` (and therefore fails the check if any of the routes
      listed in the Overview is left undocumented).
- [x] Every route listed in the Overview has a `###` section in a file under
      `docs/guides/api/` (this is what makes the coverage test pass — the
      test is the gate).
- [x] `docs/guides/api/README.md`'s map matches the actual file set after the
      REST split, and no inbound link to a removed/renamed anchor remains
      (repo-wide grep for the old anchors comes back clean).
- [x] The generated TypeScript bindings are byte-identical before and after
      (`make gen` hash comparison in `check_command`): this change is docs
      plus one test, no wire-shape edits.

### Manual / on-hardware (verified by a human before merge)

- [ ] Spot-check three newly documented endpoints against a live server
      (`make dev`) — e.g. `GET /api/version`, `GET /api/workdir/recent`, and
      one error path — and confirm the documented shapes match reality.

## Out of scope

- Generating prose from the table or doc comments automatically. The docs
  stay hand-written; only their route coverage is machine-checked.
- Documenting query parameters, status codes, or error bodies in the
  `ENDPOINTS` table itself — those stay prose-only, as the endpoint module's
  docs state.
- Any change to route behavior, handlers, or wire shapes.
