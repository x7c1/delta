---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1016-0100-fall-back-to-the-stored-token-and-notice-a-refused-websocket
created_at: 2026-10-15T16:00:00Z
updated_at: 2026-10-15T18:42:04Z
---

# fix(auth): keep a working stored token over a used link, and notice a refused WebSocket

## Overview

Two gaps in how the web app reacts to authentication, both in
`frontend/packages/apps/web/src/features/auth/` and the API client in
`frontend/packages/gateway/api-client/`.

### 1. A used link hides a token that still works

`establishAuth` (`features/auth/establishAuth.ts`) trades the one-time code in
the URL fragment for the bearer token. When the exchange answers `401` (the
code was already used, expired or unknown) it returns `code-refused`
regardless of what `localStorage` holds, so the app shows the "open a new link"
screen. Opening an old link from a bookmark or the history therefore stops the
user even though the page already holds a token the running server accepts.

Change the boot so that a refused code falls back to the stored token when
there is one: the app renders as `ready` with that token. If the stored token
turns out to be stale too, the first `401` it gets flips the app to
`token-refused` exactly as today, so the user still ends up on the screen with
the right explanation. Only when no token is stored does a refused code show
`code-refused`. A code that cannot be traded for another reason
(`exchange-failed`: the server is unreachable, storage refused) keeps today's
behaviour. Update the doc comments that describe the current rule ("a refused
code shows the screen asking for a new link, whatever is stored").

### 2. A refused WebSocket upgrade never reaches the gate

`AuthGate` swaps the app for the reopen screen when the API client's
`onUnauthorized` fires, and only REST responses fire it. The WebSocket clients
(`api-client/src/ws.ts`, `comms.ts`, `pty.ts`) present the token as `token=` on
the upgrade, and a server that refuses it answers the upgrade with `401` — but
a browser never tells the page why an upgrade failed, so the sockets just close
and keep reconnecting behind a workspace that silently stops updating.

When one of these sockets closes without having been told to, re-check
authentication over REST (a cheap authenticated request) and, if that answers
`401`, call the same `onUnauthorized` path so the gate switches to the reopen
screen. Do this once per close (do not poll), and do not let a re-check that
fails for a non-auth reason (network down, server restarting) flip the gate —
reconnecting as today is the right response to those. Keep the check in one
place that all three socket clients share rather than three copies.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A unit test boots `establishAuth` with a fragment code whose exchange
      answers `401` while a token is stored, and gets `ready` with the stored
      token kept; the same with no stored token gets `code-refused`.
- [x] A unit test shows a non-`401` exchange failure still yields
      `exchange-failed` whether or not a token is stored.
- [x] A unit test closes each of the event, comms and pty sockets unexpectedly
      with a REST re-check answering `401`, and `onUnauthorized` is called
      once; with the re-check answering `200` or failing at the network level,
      `onUnauthorized` is not called and the socket reconnects as before.
- [x] A socket closed deliberately by the app (unmount, session switch) does
      not trigger a re-check (unit test).

### Before merge (verified outside the check command)

- [x] Against a running `make dev`, opening an already-used `#code=` link in a
      tab whose `localStorage` holds the current token lands in the app, and
      after restarting the server (new token) the open app switches to the
      reopen screen within one reconnect attempt. The merging session runs
      this in a browser and records what it saw in the PR.
