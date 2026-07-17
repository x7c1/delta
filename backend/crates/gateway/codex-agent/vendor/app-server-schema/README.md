# Vendored `codex app-server` protocol schema

This directory holds the authoritative JSON Schema for the `codex app-server`
JSON-RPC protocol, generated verbatim from a pinned Codex CLI. It is the
**ground-truth reference** that Delta's Codex adapter wire types are reconciled
against: later work diffs Delta's own types (in `codex-agent`'s `wire`,
`translate`, and adapter layers) against this schema to detect drift.

## Version pin

| Field | Value |
| ----- | ----- |
| Codex CLI | `codex-cli 0.144.4` |
| Generated with | `codex app-server generate-json-schema --out <dir>` |

The version is also encoded in code as
[`codex_agent::schema::VENDORED_CODEX_VERSION`], so drift detection has a single
programmatic baseline. When you re-generate against a newer Codex, bump that
constant and re-vendor these files in the same change.

Regenerating is offline and needs no auth or network — the generator is a static
dump of the compiled-in schema.

## v1 vs v2 — why v2 is the base, plus the top-level server-request registry

The generator emits two protocol versions:

- **v1** — a legacy stub containing only the `initialize` handshake
  (`InitializeParams`, `InitializeResponse`); 2 files. It does **not** describe
  the structured conversation protocol. It is intentionally **not vendored**
  here beyond this note.
- **v2** — the real conversation protocol: `thread/*`, `turn/*`, `item/*`, and
  server/client notifications; 228 individual files plus the combined document.
  Delta pins **v2** for the client-request + notification surface.

### The v2 combined document OMITS the server → client request registry

Empirically confirmed against a live `codex app-server 0.144.4` turn: for a
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
