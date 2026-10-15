---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, error-type-design]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1016-0103-bound-websocket-frames-connections-and-transcript-reads
created_at: 2026-10-15T16:03:00Z
updated_at: 2026-10-15T16:41:27Z
---

# fix(server): bound WebSocket frames and connections, and transcript reads

## Overview

Two places let a single input consume memory or block work without limit.
Neither is reachable without the bearer token or the agent's own files, so
these are hardening, not emergencies; the fix is to put explicit, documented
bounds where today there are none.

### WebSocket frames and connections

The three WebSocket endpoints in `backend/crates/apps/delta-server/src/` —
`ws.rs` (events), `comms.rs` (comms frames) and `pty.rs` (terminal bridge) —
accept upgrades with axum's defaults (64 MiB messages, 16 MiB frames) and with
no cap on how many sockets a client may hold open. A token holder can exhaust
memory with huge frames or many sockets.

- Set `max_message_size` / `max_frame_size` on each upgrade to a bound sized
  for what that channel actually carries (the events and comms channels are
  server → client and should accept almost nothing inbound; the pty channel
  carries keystrokes and pastes, so allow a generous paste, e.g. 1 MiB). Name
  each bound as a constant with a comment saying why that size.
- Cap concurrent connections: per session for `pty` and `comms` (a small
  number such as 8 covers multiple tabs), and a global cap for the events
  socket (e.g. 32). Over the cap, refuse the upgrade with a clear status
  (`429` or `503`) and a `warn` log naming the endpoint and the cap; release
  the slot when a socket ends for any reason (normal close, error, panic in the
  pump), so a crashed socket never leaks a slot.

### Transcript reads

`backend/crates/gateway/delta-transcript/src/reader/mod.rs` reads the whole
transcript JSONL with `fs::read_to_string(path)` on every hook. A huge file
makes every hook proportional to its size; a FIFO placed inside the transcript
root blocks the blocking thread and the session actor behind it; one invalid
UTF-8 byte stops ingestion of the whole file.

- Open the file non-blocking and refuse anything that is not a regular file
  (a FIFO, a device, a directory) with a typed error, then read normally.
- Refuse a file over a size cap (a named constant with its rationale; pick a
  value comfortably above real long sessions, e.g. 256 MiB) with a typed error
  that is logged once per hook at `warn`, not silently treated as empty.
- Decode line by line so one invalid UTF-8 line is skipped (with a `debug`
  log carrying the line number) and the rest of the transcript still ingests.
- Reading only the new bytes since the last offset is out of scope here (a
  larger change to the reader's contract); leave a note at the read site.

Update `docs/guides/security.md` where it describes the WebSocket endpoints and
transcript ingestion so the new bounds are stated.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] For each of the three WebSocket endpoints, a test shows a message over
      its bound closes the socket (or is rejected) without the server
      buffering it, and a message under the bound is delivered.
- [x] A test opens connections up to the cap on one endpoint, sees the next
      upgrade refused with the documented status, then closes one and sees a
      new upgrade accepted.
- [x] A test shows a socket whose pump ends with an error releases its slot.
- [x] Reader tests: a FIFO path returns the not-a-regular-file error without
      blocking (the test completes under a timeout); a file over the cap
      returns the too-large error; a file with one invalid UTF-8 line yields
      every other line.

### Before merge (verified outside the check command)

- [x] `make dev`: a normal session (send, stream, terminal pane, paste of a
      few KB) behaves as before. The merging session runs it and records the
      result in the PR.
