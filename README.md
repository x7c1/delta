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

Download the desktop app for your platform from the
[latest Release](https://github.com/x7c1/delta/releases/latest) — a `.dmg`
for macOS (Apple silicon or Intel), a `.deb` or an `.AppImage` for Linux —
open it, and start a session from the composer. The app needs `tmux` and an
authenticated `claude` and/or `codex` on the host. The builds are unsigned, so
macOS blocks the first launch:
[docs/guides/install.md](docs/guides/install.md) has the steps past that, the
Linux notes, and where the app keeps its data.

To work on Delta itself, run it from source:

```
git clone https://github.com/x7c1/delta.git
cd delta
make dev
```

`make dev` brings up the local development loop (backend + frontend), and
`make help` lists every other target.

- Prerequisites and the day-to-day workflow:
  [docs/guides/development](docs/guides/development/README.md)
- The browser↔server contract:
  [docs/guides/api](docs/guides/api/README.md)
