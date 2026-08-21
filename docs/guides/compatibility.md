# Compatibility policy

## Overview

Delta is currently on `0.x`. This document is the source of truth for what
"no compatibility is guaranteed" (as the top-level README puts it) means in
practice, broken down by the three surfaces where a compatibility promise
could plausibly apply:

1. **SQLite schema** — the on-disk overlay (`delta.db`) that delta-server
   maintains alongside the agent CLIs.
2. **Wire contract** — the REST, WebSocket, and hook shapes shared between
   delta-server and the browser UI (defined by the `delta-wire` crate and
   regenerated into `@delta/wire-gen`).
3. **Agent CLI compatibility** — which versions of the upstream agent CLIs
   (`claude`, `codex`) delta is known to work against.

All three are governed by the same `v0.x` stance: *free to break, optimised
for development velocity, re-decided at `v1.0`*. The one qualification is
the on-disk overlay: its shape is still free to change, but a change that an
existing database cannot absorb has to carry it forward, because half of
what the overlay holds exists nowhere else. The rationale, the operational
safety net, and the rule's intended expiry differ per surface, and the rest
of this document spells each one out.

Delta's current user base is a single developer (the maintainer himself).
The `v0.x` phase is deliberately scoped to that reality: there is no
deployed fleet, no third-party integration, and no published binary
distribution where an old client could be running against a new server. The
rules below take advantage of that scope. They are expected to tighten when
the user base widens or when `v1.0` cutover is on the table.

For build, run, and `make reset` mechanics referenced below, see the
[development guide](development/README.md); this document only covers the
policy layer, not the workflow itself.

## Subdomain 1 — SQLite schema

### Policy (v0.x)

The schema may still change in any shape — dropping a column or table,
renaming, tightening a constraint, changing a declared type, replacing an
index strategy. What is no longer free is the *upgrade path*: **a change
that an existing database cannot absorb ships a migration step that carries
it forward.** `make reset` is an escape hatch for the rare change that
genuinely cannot be migrated, not the way delta is upgraded.

The schema is defined by a migration ladder in
`backend/crates/gateway/delta-sqlite/src/migrations/`: an ordered list of
steps, each carrying the `PRAGMA user_version` it produces, grouped into one
module per schema subject. A change appends a step and bumps
`SCHEMA_VERSION`; opening a database applies every step above the version the
file is stamped with. A *fresh* database is built by replaying the whole
ladder, so there is no second definition of the schema to keep in sync — see
the module docs for the mechanics, including the pre-migration snapshot a
destructive step triggers.

### Why the upgrade path matters

Delta is a wrapper around the agent CLIs, not the system of record, and half
of what the overlay holds reflects that — message content, the linear parent
chain, and per-message metadata are a **cache**, rebuildable from Claude
Code's transcript JSONL. The records they are derived from live on the agent
side, and nothing delta does to its overlay touches them:

- Claude Code's transcript JSONL and hook payloads.
- Codex's thread storage, owned by `codex app-server`.
- The session bodies themselves.

But the other half exists **only** in the overlay and cannot be rebuilt from
anything: the thread structure (`thread_id`, `semantic_parent_uuid`, the
thread rows themselves), the outgoing-send queue, the permission decision
history, the registered launch options and clone roots. `make reset` deletes
all of that. Every branch the user has ever made, and every send still
waiting to be dispatched, is gone — and no re-ingest brings it back. The
cost of a reset is therefore **not** low, which is why the ladder exists.

### Operational safety net: the startup gate

A monotonically incremented `SCHEMA_VERSION` constant lives in the
`delta-sqlite` crate and is reflected into the SQLite file via
`PRAGMA user_version`. On startup, delta-server compares the binary's
expected version to the value stored in the DB:

- **Match.** Continue normally: no step is applied and nothing is written.
- **Below, but at or above the ladder's oldest step.** Apply the pending
  steps, one transaction per version, and continue. If any pending step is
  destructive, a snapshot (`delta.db.bak-v<source version>`) is written first
  and never deleted automatically.
- **Below the ladder's oldest step.** The versions under the squashed
  baseline (1 and 2 — every overlay `v0.2.x` or `v0.3.0` wrote is stamped 1)
  have no steps, so nothing carries such a file forward, and replaying the
  baseline over it would apply nothing and then stamp the older shape as
  current. Refuse to start with an error naming `make reset`.
- **Above.** The DB was written by a newer binary. Refuse to start, exit
  non-zero, and print an error naming the remediation.
- **No stamp at all, but delta's tables present.** A pre-gate `v0.1.0`
  overlay, whose real shape is unknown. Refuse to start with an error naming
  `make reset`.

Downgrade — running an *older* binary against a *newer* DB — is best-effort,
with no formal promise: the ladder only runs forward, so an older binary
seeing a higher `user_version` refuses to start.

### Commit and release-note convention

When a change genuinely cannot be migrated — i.e. it requires `make reset`
to upgrade, because no forward step can reconstruct what the new shape needs:

- The **commit message** must say `make reset required` explicitly.
- The **release notes** for the version that ships it must repeat the
  same phrase.

This is the only documentation of which versions need a reset, so it has
to be searchable on a verbatim string. A destructive change that *does* ship
a migration step is an ordinary change and carries no such marker — the
phrase now marks the exception, not the routine.

### When this rule expires

This policy is re-decided when **either** of:

- The user base expands beyond the single-developer maintainer.
- A `v1.0` cutover is scheduled.

After `v1.0`, expect the rule to tighten further: a documented migration for
every schema change, and `make reset` withdrawn as an accepted answer even
for the exceptional case.

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

## Subdomain 3 — agent CLI compatibility

### Policy (v0.x)

Delta publishes **nothing** about which versions of the upstream agent
CLIs (`claude`, `codex`) it supports. There is no published version range,
no `Verified-against:` marker, no last-green pin. Both upstreams move
fast — Claude Code sometimes ships several updates per day — and any
hand-maintained range would go stale faster than it can be reviewed;
rather than commit to a freshness obligation that would pull in a cron
or timer driver to satisfy, `v0.x` simply makes no public statement at
all.

This is symmetric with subdomains 1 and 2: in `v0.x`, delta promises
nothing about any of the three surfaces. The only on-disk record of
what `claude` version a given installation actually ran against is the
startup info log described below; there is no `codex` counterpart yet.

### Startup version log

At startup, delta-server logs the output of `claude --version` at info
level. There is **no** enforcement: delta does not refuse to start, does
not warn, and does not perform any version compatibility check against
the running binary. The log line exists purely as an observability hook,
so that a later breakage report can be correlated with the specific
upstream `claude` version a given run was using.

The behaviour and failure semantics (info on success, warn on
spawn-failure or non-zero exit, startup continues in every case) are
defined by the doc-comment on `log_claude_version` in
`backend/crates/apps/delta-server/src/claude_version.rs`, which is the
source of truth.

There is no `codex` counterpart to this log yet. The server's
provider-availability probe reports **binary presence only** (it is what
the new-session UI uses to surface an unlaunchable provider), and a
version-compatibility verdict is deferred to the real-Codex canary — see
the doc-comment on `ProviderAvailability` in
`backend/crates/domain/delta-model/src/provider_availability.rs`.

### Legacy-format parsing

`delta-transcript` and `delta-attribution` contain branches labelled as
**legacy-format compatibility** — paths that keep older Claude Code
transcript shapes parseable so existing recorded transcripts can still
be viewed and resumed. The current example is the pre-`queue-operation`
queued-prompt shape (see the drift runbook in
[development/canary.md](development/canary.md) for context).

Symmetric with the "freely break" stance of subdomains 1 and 2, these
legacy branches **may be removed at any time during `v0.x`**. There is no
deprecation window. If a legacy branch is removed, transcripts recorded
under that older Claude Code shape will no longer be readable; the
mitigation, when needed, is to re-record them. Note this is not a `make
reset`: what breaks is the parse of an upstream transcript, and the overlay
built from it is left alone.

### Fix window on upstream breakage

When an upstream agent CLI release breaks delta's parsing, hook handling,
or app-server integration, the fix window is **best-effort**. Delta does
not promise a turnaround time, and does not commit to any scheduled
detection mechanism. In practice the fix happens when the maintainer notices the
breakage during their own use of delta, not on a schedule and not in
response to a public canary signal.

### `scripts/e2e-real-auto.sh` as a retained tool

`scripts/e2e-real-auto.sh` is a gating wrapper around the real-claude
canary suite (`make e2e-real`) that runs the canary only when both the
installed `claude --version` differs from the version recorded at the
last attempt **and** at least 24 hours have passed since the last
attempt. The wrapper and its per-host state files are kept in the tree
as an **internal development tool**: the maintainer can invoke it by
hand, or attach it to a periodic driver locally, when it is useful. It
is not part of any public compatibility commitment — `v0.x` does not
promise to run it on a schedule, and does not publish its results.

For the gating mechanism, the per-host state files, and an optional
periodic-driver setup, see
[development/canary.md — Automatic canary trigger](development/canary.md#automatic-canary-trigger-opt-in).

The Codex counterpart is `make e2e-real-codex`, which runs the real-codex
canaries (one safe turn end to end, the thread-metadata wire fields, and
schema drift detection) against the real `codex app-server`. It has no
auto-gating wrapper yet, and is likewise an internal tool, not a public
compatibility commitment.

### When this rule expires

This policy — publishing nothing about upstream agent CLI versions, the
no-enforcement startup log, the freedom to remove legacy branches, the
best-effort fix window, and the absence of any scheduled canary
contract — is re-decided at `v1.0`, or earlier if the user base expands
beyond a single developer. At that point a published version range and
some form of automated update mechanism are likely to be re-evaluated.

## Summary: what carries forward to v1.0

| Rule | v0.x | Expected at v1.0 |
|------|------|------------------|
| Destructive SQLite schema changes | Free in shape, but ship a migration step | Kept; a documented migration for every schema change |
| `SCHEMA_VERSION` gate + forward migration on startup | Shipped | Kept |
| `make reset` as the upgrade path | Escape hatch for the change that cannot be migrated | Withdrawn |
| Wire-breaking changes (REST / WS / hooks) | Free | Versioned contract; breaking changes annotated |
| `Accept-Version` / `/ws` handshake version | None | Likely introduced |
| Wire bindings freshness CI gate | Kept | Kept |
| Published agent CLI version ranges | None | Re-decided |
| Startup `claude --version` log | Info, no enforcement | Re-decided |
| Removal of legacy transcript-format branches | Free | Re-decided (likely deprecation window) |
| Fix window on upstream agent CLI breakage | Best-effort | Re-decided |

The single principle behind all of this: **`v0.x` is the phase where
delta optimises for iteration speed, knowing that the only user is the
developer in front of it.** Every rule above is a deliberate choice to
keep that phase cheap, with the safety nets (the schema gate and its
forward migrations, the canary suite, the startup version log) sized for
*one developer debugging one machine*, not for a deployed fleet. The one
place cheapness stops applying is that developer's own overlay: it is a
working machine's live state, and the schema ladder is what keeps a schema
change from costing it.
