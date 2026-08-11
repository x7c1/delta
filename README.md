# Delta

A local browser tool for driving AI coding agent sessions.

- **Branch anywhere** — fork a side thread from any past message, dig in,
  and return to the main line without losing your place.
- **Readable transcripts** — a conversation viewer built for reading, far
  more comfortable than scrolling a terminal.
- **All sessions side by side** — browse, compare, and resume any past
  conversation at a glance.
- **Multiple agents, one UI** — run Claude Code and Codex sessions from the
  same workspace.

The name comes from a river delta: the way a conversation forks from its main
channel into side branches.

## Status

Delta is alpha quality. It adopts
[Semantic Versioning](https://semver.org/spec/v2.0.0.html), but while it stays
on `0.x` **no compatibility is guaranteed**: the SQLite schema, the
browser↔server wire contract, and which upstream agent CLI versions Delta
works against may all change in any `0.x` bump. See
[docs/guides/compatibility.md](docs/guides/compatibility.md) for the full
policy. Supported platforms are **Linux and macOS**.

## Install

Delta is distributed as source only — there are no prebuilt binaries yet.

```
git clone https://github.com/x7c1/delta.git
cd delta
make dev
```

`make dev` runs the local development loop (backend + frontend). See
[docs/guides/development.md](docs/guides/development.md) for prerequisites
(tmux, cargo, pnpm, and the agent CLIs you plan to use) and the rest of the
workflow.

## Development

Common tasks run through `make` from the repo root — `make help` lists the
targets (`dev`, `mock`, `build`, `test`, `lint`, `check`, `e2e`, …). Build,
test, lint, and run details are in
[docs/guides/development.md](docs/guides/development.md), and the
browser↔server contract in [docs/guides/api.md](docs/guides/api.md).
