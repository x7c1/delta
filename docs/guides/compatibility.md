# Compatibility policy

## Overview

Delta is currently on `0.x`. This document is the source of truth for what
"no compatibility is guaranteed" (as the top-level README puts it) means in
practice, broken down by the three surfaces where a compatibility promise
could plausibly apply:

1. **SQLite schema** — the on-disk overlay (`delta.db`) that delta-server
   maintains alongside Claude Code.
2. **Wire contract** — the REST, WebSocket, and hook shapes shared between
   delta-server and the browser UI (defined by the `delta-wire` crate and
   regenerated into `@delta/wire-gen`).
3. **`claude` CLI compatibility** — which versions of the upstream Claude
   Code CLI delta is known to work against.

All three are governed by the same `v0.x` stance: *free to break, optimised
for development velocity, re-decided at `v1.0`*. The rationale, the
operational safety net, and the rule's intended expiry differ per surface,
and the rest of this document spells each one out.

Delta's current user base is a single developer (the maintainer himself).
The `v0.x` phase is deliberately scoped to that reality: there is no
deployed fleet, no third-party integration, and no published binary
distribution where an old client could be running against a new server. The
rules below take advantage of that scope. They are expected to tighten when
the user base widens or when `v1.0` cutover is on the table.

For build, run, and `make reset` mechanics referenced below, see
[development.md](development.md); this document only covers the policy
layer, not the workflow itself.

## Subdomain 1 — SQLite schema

### Policy (v0.x)

Schema changes may be **freely destructive**. There is no "additive-only"
rule. Specifically permitted without ceremony during `0.x`:

- `DROP` of a column or table whose meaning changed.
- Renaming a column or table.
- Tightening a constraint (NOT NULL, UNIQUE, FK).
- Changing a column's declared type.
- Replacing an index strategy.

Schema changes do **not** have to ship a forward migration. The expected
upgrade path for a destructive change is `make reset`, which rebuilds the
overlay from scratch.

### Why this is safe

Delta is a wrapper around Claude Code, not the system of record. The data
that would be painful to lose lives on the Claude Code side:

- The transcript JSONL.
- The Claude Code hook payloads.
- The session bodies themselves.

Delta's SQLite overlay holds only the metadata it derives or layers on top
of that — thread structure, pending-send correlation, recency hints, and
similar. `make reset` deletes that overlay and nothing else; the Claude
Code data is untouched, so after a reset the next run hydrates from the
upstream transcripts on first use. The user-visible cost of `make reset`
is therefore low, and `v0.x` deliberately exploits that to keep schema
iteration cheap.

### Operational safety net: `SCHEMA_VERSION` gate

A monotonically incremented `SCHEMA_VERSION` constant lives in the
`delta-sqlite` crate and is reflected into the SQLite file via
`PRAGMA user_version`. On startup, delta-server compares the binary's
expected version to the value stored in the DB:

- **Match.** Continue normally.
- **Mismatch.** Refuse to start, exit non-zero, and print an error that
  names `make reset` as the remediation.

The point of the gate is to fail loud and early on the only realistic
failure mode (a stale overlay against a newer binary), instead of letting
the mismatch surface as confusing runtime errors much later in a session.

Downgrade — running an *older* binary against a *newer* DB — is
best-effort, with no formal promise. The gate naturally protects this
direction too: an older binary expecting a lower version sees the higher
`user_version` and refuses to start, again pointing at `make reset`.

**Implementation status.** The `SCHEMA_VERSION` gate is scheduled in a
follow-up PR. Until that ships, a stale DB surfaces as runtime errors
rather than a clean startup exit; the policy in this document is still
in force, the early-exit machinery just is not there yet.

### Existing additive machinery

The `ADDITIVE_COLUMNS` table and `apply_additive_columns` machinery in
`backend/crates/gateway/delta-sqlite/` are kept as-is. They remain useful
for genuinely additive evolutions (where a `make reset` would be
gratuitous), but during `v0.x` they are not promoted to a constraint —
nothing forces a schema change to go through that path.

### Commit and release-note convention

When a change is destructive (i.e. requires `make reset` to upgrade
cleanly):

- The **commit message** must say `make reset required` explicitly.
- The **release notes** for the version that ships it must repeat the
  same phrase.

This is the only documentation of which versions need a reset, so it has
to be searchable on a verbatim string.

### When this rule expires

This policy is re-decided when **either** of:

- The user base expands beyond the single-developer maintainer.
- A `v1.0` cutover is scheduled.

After `v1.0`, expect the rule to tighten toward an additive-only
default, with destructive changes requiring an explicit migration.

## Subdomain 2 — Wire contract

### Policy (v0.x)

Wire-breaking changes are **unrestricted** during `v0.x`. Specifically
permitted without ceremony:

- Removing a REST field.
- Changing a field's type.
- Removing or renaming an endpoint.
- Adding, removing, or restructuring a `SessionEvent` variant on `/ws`.
- Changing a `/hooks/*` request or response shape.

And, as direct consequences:

- No `Accept-Version` request header is required or honoured.
- The `/ws` upgrade does **not** carry a handshake version exchange.
- A wire-breaking change does **not** require a `feat!:` /
  `BREAKING CHANGE:` annotation in the commit.
- Release notes do **not** carry a "Breaking changes" category for wire
  shape changes.

### Why this is safe

The wire contract is an **internal implementation extension**, not an
external contract. The architecture makes that real, not aspirational:

- delta-server is a local binary that serves the frontend assets from the
  same process.
- Across every distribution form on the roadmap — source-only,
  `cargo-binstall`, prebuilt release artifact, eventual Tauri shell — the
  frontend and backend are always produced from the same checkout (and,
  in the binary forms, the same binary).
- There is no scenario where a user runs frontend version *X* against
  backend version *Y*. They ship as one unit, locally, every time.

Lockstep here is a property of the local-launch architecture, not a
promise made by a deploy procedure. There is no third-party wire
consumer that would need a stable shape.

### What the existing CI gate is (and is not)

A CI step **`Check generated wire bindings are fresh`** keeps the
TypeScript bindings (`@delta/wire-gen`) in sync with the `delta-wire`
Rust types. This is **not** a breaking-change gate; it only catches a
forgotten regeneration. With breaking changes free during `v0.x`, the
gate's role is purely "did you remember to run `make gen` after editing
the Rust types and commit the regenerated TS." That gate stays.

### When this rule expires

This policy is re-decided when **either** of:

- A real third-party consumer of the wire contract appears (anything
  other than delta's own frontend produced from the same checkout).
- A `v1.0` cutover is scheduled.

After `v1.0`, expect a versioned wire contract and conventional commit
annotations for breaking changes.

## Subdomain 3 — `claude` CLI compatibility

### Policy (v0.x)

Delta does **not** publish a supported version range for the upstream
`claude` CLI. Claude Code's own release cadence is fast — sometimes
several updates per day — and any hand-maintained range would go stale
faster than it can be reviewed.

Instead, delta publishes a single **last-green** marker:

```
Verified-against: claude X.Y.Z (last green: YYYY-MM-DD)
```

This line lives in the repository README. It is updated semi-automatically
by the canary infrastructure described below, not by hand. The marker is
the only compatibility statement delta makes for the upstream CLI.

### How "last-green" is maintained

`scripts/e2e-real-auto.sh` is a gating wrapper around the real-claude
canary suite (`make e2e-real`). On each invocation it runs the canary
only when **both**:

- The installed `claude --version` differs from the version recorded at
  the last attempt, and
- At least 24 hours have passed since the last attempt.

When the canary passes, the `Verified-against:` line in the README is
rewritten to point at the freshly verified version and date, and the
update is committed and pushed.

**Implementation status.** The version-bump, commit, and push step is
scheduled in a follow-up PR. Until that ships, the gating wrapper still
runs (and still records last-attempt state per host); the README line is
just maintained manually in the interim. The contract documented here —
"the marker reflects the most recent passing canary against the
currently-installed claude" — is what the automation will enforce.

For the full gating mechanism, the per-host state files, and the
periodic-driver setup, see
[development.md — Automatic canary trigger](development.md#automatic-canary-trigger-opt-in).

### Startup version log

At startup, delta-server logs the output of `claude --version` at info
level. There is **no** enforcement: delta does not refuse to start, does
not warn, and does not perform any version compatibility check against
the running binary. The log line exists to make post-hoc debugging
straightforward when a real-claude breakage is reported.

**Implementation status.** The startup version log is scheduled in a
follow-up PR (the same one that lands the `SCHEMA_VERSION` gate, where
practical). Until then, the runtime simply does not record the version
it ran against; a user reporting a breakage will be asked for it
manually.

### Legacy-format parsing

`delta-transcript` and `delta-attribution` contain branches labelled as
**legacy-format compatibility** — paths that keep older Claude Code
transcript shapes parseable so existing recorded transcripts can still
be viewed and resumed. The current example is the pre-`queue-operation`
queued-prompt shape (see development.md's drift runbook for context).

Symmetric with the "freely break" stance of subdomains 1 and 2, these
legacy branches **may be removed at any time during `v0.x`**. There is no
deprecation window. If a legacy branch is removed, transcripts recorded
under that older Claude Code shape will no longer be readable; the
mitigation, when needed, is the same `make reset` plus re-record cycle
the SQLite policy already implies.

### Fix window on upstream breakage

When a real-claude canary fails (i.e. an upstream Claude Code release
broke delta's parsing or hook handling), the fix window is
**best-effort**. Delta does not promise a turnaround time. The canary is
the *detection* mechanism, not a service-level commitment about how
quickly the breakage gets repaired — that depends on maintainer
bandwidth.

### When this rule expires

This policy — the absence of a published range, the no-enforcement
startup log, the freedom to remove legacy branches, the best-effort fix
window — is re-decided at `v1.0`.

## Summary: what carries forward to v1.0

| Rule | v0.x | Expected at v1.0 |
|------|------|------------------|
| Destructive SQLite schema changes | Free | Tightened toward additive-only by default |
| `SCHEMA_VERSION` gate on startup | Required (scheduled) | Kept, plus formal migrations for destructive changes |
| `make reset` as the upgrade path | Acceptable for any change | Acceptable only for the few explicitly destructive bumps |
| Wire-breaking changes (REST / WS / hooks) | Free | Versioned contract; breaking changes annotated |
| `Accept-Version` / `/ws` handshake version | None | Likely introduced |
| Wire bindings freshness CI gate | Kept | Kept |
| Published `claude` version range | None | Re-decided |
| Last-green `Verified-against:` README marker | Authoritative | Likely kept, may co-exist with a stricter pin |
| Startup `claude --version` log | Info, no enforcement | Re-decided |
| Removal of legacy transcript-format branches | Free | Re-decided (likely deprecation window) |
| Fix window on upstream `claude` breakage | Best-effort | Re-decided |

The single principle behind all of this: **`v0.x` is the phase where
delta optimises for iteration speed, knowing that the only user is the
developer in front of it.** Every rule above is a deliberate choice to
keep that phase cheap, with the safety nets (the `SCHEMA_VERSION` gate,
the canary suite, the startup version log) sized for *one developer
debugging one machine*, not for a deployed fleet.
