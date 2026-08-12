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

Delta is alpha quality.

- Supported platforms: **Linux** and **macOS**.
- While it stays on `0.x`, **no compatibility is guaranteed** — see
  [docs/guides/compatibility.md](docs/guides/compatibility.md) for what that
  means for each surface.

## Getting started

Delta is distributed as source only — there are no prebuilt binaries yet.

```
git clone https://github.com/x7c1/delta.git
cd delta
make dev
```

`make dev` brings up the local development loop (backend + frontend), and
`make help` lists every other target.

- Prerequisites and the day-to-day workflow:
  [docs/guides/development.md](docs/guides/development.md)
- The browser↔server contract:
  [docs/guides/api.md](docs/guides/api.md)
