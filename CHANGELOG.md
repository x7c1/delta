# Changelog

All notable changes to Delta will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and Delta adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
from v0.1.0 onward.

> Note on compatibility: while Delta is at 0.x, **no compatibility is
> guaranteed** — schema migrations, the browser↔server wire contract, and the
> supported Claude CLI version range may all change in any 0.x release.

## [Unreleased]

## [0.1.0] - 2026-06-19

First tagged release. Alpha quality, Linux only, source distribution
(`git clone` + `make dev`) only.

### Added

- Multi-session management: spawn, view, and resume Claude Code sessions, each
  driven through its own tmux pane on a dedicated tmux socket.
- Thread navigation within a session: branch off any past message into a side
  thread, dig in, and return to the main line without losing your place.
- Conversation viewer with a cursor-paginated session list and DOM-virtualized
  sub-thread trees that stay responsive as history grows.
- Embedded terminal (xterm.js) for answering Claude Code permission prompts
  directly in the browser, with an in-UI dedicated card for `AskUserQuestion`.
- Live streaming of assistant turns into the conversation pane.
- Queued send cancellation: drop a pending send before it is dispatched.
- Settings screen with a custom launch-option registry (CRUD over a REST API)
  and per-session option pre-check in the start picker.
- StatusLine mirror surfaced in the UI: context usage, rate limits, and
  per-message metadata (model, working directory, branch, response time).
- Subagent activity visualization: a running-subagent indicator and
  thread-aware running and unread indicators in the navigator.
- Per-session git worktree isolation, opt-in when starting a new session.
- Choose a working directory for new sessions, with a directory-list endpoint
  and recent-cwd suggestions.
- Backend over HTTP + WebSocket on `127.0.0.1` only, with a SQLite overlay
  persisting the thread structure on top of Claude Code's JSONL transcripts.
- Generated TypeScript bindings for the REST and WebSocket wire contracts.
- Headless Playwright e2e suites: a fake-claude lane for deterministic
  full-loop coverage and a real-claude canary lane.

[Unreleased]: https://github.com/x7c1/delta/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/x7c1/delta/releases/tag/v0.1.0
