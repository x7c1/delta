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

## v1 vs v2 — why only v2 is vendored

The generator emits two protocol versions:

- **v1** — a legacy stub containing only the `initialize` handshake
  (`InitializeParams`, `InitializeResponse`); 2 files. It does **not** describe
  the structured conversation protocol. It is intentionally **not vendored**
  here beyond this note.
- **v2** — the real protocol: `thread/*`, `turn/*`, `item/*`, approvals, and
  server/client notifications; 228 individual files plus the combined document.
  Delta pins **v2**; the whole structured protocol Delta uses is v2.

## Files

- `codex_app_server_protocol.v2.schemas.json` — the **authoritative** combined
  v2 document (`title: CodexAppServerProtocolV2`, one `definitions` map with
  every type). This is the required artifact reconciliation validates against.
- `v2/*.json` — the same v2 protocol split into one file per type. Vendored
  purely for convenience: per-type diffs make it easy to see, in review, exactly
  which type moved when the schema is re-generated against a newer Codex. The
  combined file above remains the source of truth.

The generator's other outputs are deliberately **not** vendored: the v1 files
(legacy, see above), the non-versioned combined `codex_app_server_protocol.schemas.json`,
and the loose top-level per-type files (superseded by the v2 combined document).
