# Vendored `codex app-server` protocol schema

## Overview

This directory holds the authoritative JSON Schema for the `codex app-server`
JSON-RPC protocol, generated from a pinned Codex CLI and stored key-sorted (see
[Stored form](#stored-form-generated-then-key-sorted)). It is the
**ground-truth reference** that Delta's Codex adapter wire types are reconciled
against: later work diffs Delta's own types (in `codex-agent`'s `wire`,
`translate`, and adapter layers) against this schema to detect drift.

## Version pin

| Field | Value |
| ----- | ----- |
| Codex CLI | `codex-cli 0.153.4` |
| Generated with | `make vendor-codex-schema` |

The version is also encoded in code as
[`codex_agent::schema::VENDORED_CODEX_VERSION`], so drift detection has a single
programmatic baseline. When you re-generate against a newer Codex, bump that
constant in the same change (see [Re-vendoring](#re-vendoring)).

## Stored form: generated, then key-sorted

These files are not the generator's bytes. Each one is `jq -S -j .` of the
generator's output — sorted keys, two-space indent, no trailing newline.

The generator's output order is **not stable**: the same Codex version can emit
the same definitions in a different order from one run to the next. Vendored as
emitted, a re-vendor would therefore produce a diff full of reorderings, with
the handful of real protocol changes buried in it; sorting the keys makes the
diff show what actually changed. Nothing depends on the byte layout — the drift
canary below compares definition key sets and definition values, not bytes, so
the normalisation is invisible to it.

`make vendor-codex-schema-check` fails when a file here stops being its own
`jq -S -j .`. It needs only `jq`, and both `make check` and CI's backend job run
it.

## Re-vendoring

```bash
make vendor-codex-schema    # DELTA_CODEX_BIN overrides the codex binary
```

`scripts/vendor-codex-schema.sh` runs the generator into a temp directory, keeps
exactly the outputs listed under [Files](#files) below, normalises each of them,
and replaces the files here — removing any the generator no longer emits, and
leaving this README alone. It prints the generator's version and, when that is
not the pin above, reminds you to bump `VENDORED_CODEX_VERSION` and the version
pin table in the same change. Re-vendoring is offline and needs no auth or
network — the generator is a static dump of the compiled-in schema.

## Drift detection

Drift is guarded by the `#[ignore]` canary
`vendored_schema_matches_the_real_generator` in
`codex-agent/tests/real_codex_canary.rs` (run via `make e2e-real-codex`): it
regenerates the schema with the installed `codex` and fails loudly if the
generator's output — the definition sets and the definitions Delta reconciled
against — diverges from these vendored files or from the pinned version. A
failure is the signal to re-vendor and bump the constant.

## v1 vs v2 — why v2 is the base, plus the top-level server-request registry

The generator emits two protocol versions:

- **v1** — a legacy stub containing only the `initialize` handshake
  (`InitializeParams`, `InitializeResponse`); 2 files. It does **not** describe
  the structured conversation protocol. It is intentionally **not vendored**
  here beyond this note.
- **v2** — the real conversation protocol: `thread/*`, `turn/*`, `item/*`, and
  server/client notifications; 265 individual files plus the combined document.
  Delta pins **v2** for the client-request + notification surface.

### The v2 combined document OMITS the server → client request registry

Empirically confirmed against a live `codex app-server 0.153.4` turn: for a
`turn/start` turn the server drives approvals as **server → client requests**
(request/response with an id), not notifications — e.g.
`item/commandExecution/requestApproval`, answered `{"decision":"decline"}`.

That `ServerRequest` registry is **not** in `codex_app_server_protocol.v2.schemas.json`
(it has no `ServerRequest` / `*RequestApprovalParams` definitions). The generator
instead emits it in the **non-versioned** combined document
(`codex_app_server_protocol.schemas.json`, `title: CodexAppServerProtocol`) and
as loose top-level per-type files. PR #267 vendored only the v2 combined file, so
the whole approval surface was missing; this directory now vendors it (below).

## Files

- `codex_app_server_protocol.v2.schemas.json` — the combined v2 document
  (`title: CodexAppServerProtocolV2`): the client-request + notification surface.
- `codex_app_server_protocol.schemas.json` — the combined **non-versioned**
  document (`title: CodexAppServerProtocol`). This is the authoritative
  ground-truth reference for the **server → client request** surface: it is the
  only combined document that carries the `ServerRequest` `oneOf` registry (and
  its `*RequestApprovalParams` / `*RequestApprovalResponse` types). Reconciliation
  of the approval fan-out validates against this file.
- `ServerRequest.json` — the standalone `ServerRequest` `oneOf`: every method the
  server can request of the client, with its params type. The approval methods
  Delta cares about are `item/commandExecution/requestApproval`,
  `item/fileChange/requestApproval`, and `item/permissions/requestApproval`.
- `CommandExecutionRequestApprovalParams.json` / `…Response.json` — the
  command-execution approval request params and its `{decision}` response
  (`CommandExecutionApprovalDecision` =
  `accept | acceptForSession | acceptWithExecpolicyAmendment |
  applyNetworkPolicyAmendment | decline | cancel`).
- `FileChangeRequestApprovalParams.json` / `…Response.json` — the file-change
  approval request params and its `{decision}` response
  (`FileChangeApprovalDecision` = `accept | acceptForSession | decline | cancel`).
- `PermissionsRequestApprovalParams.json` / `…Response.json` — the permissions
  approval request. Its response is **not** a binary decision: it returns a
  `GrantedPermissionProfile` (`{permissions, scope?, strictAutoReview?}`). Delta
  has no neutral projection for a granted permission profile, so v1 surfaces this
  method as an `UnsupportedInteraction` rather than fabricating a grant (see
  `codex_agent::translate` / `adapter`).
- `v2/*.json` — the v2 protocol split into one file per type. Vendored purely for
  convenience: per-type diffs make it easy to see, in review, exactly which type
  moved when the schema is re-generated against a newer Codex.

The generator's other outputs are deliberately **not** vendored: the v1 files
(legacy, see above) and the loose top-level per-type files other than the
server-request/approval ones listed above (superseded by the combined documents).
`scripts/vendor-codex-schema.sh` encodes exactly this selection, so a re-vendor
applies it for you; changing what is vendored means changing both the list above
and the one in that script.
