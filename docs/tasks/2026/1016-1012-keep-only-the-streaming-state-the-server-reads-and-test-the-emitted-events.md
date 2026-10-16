---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! git grep -nE 'fn streaming_message\\(|fn text\\(&self\\)|chunks: Vec<\\(u32, String\\)>' -- backend/crates/domain/delta-usecase/src/interactor/session_actor/runtime && ! git grep -n 'running text so the caller' -- backend/crates/domain/delta-usecase"
assignee: null
branch: task/1016-1012-keep-only-the-streaming-state-the-server-reads-and-test-the-emitted-events
created_at: 2026-10-16T10:11:14Z
updated_at: 2026-10-16T16:25:56Z
---

# refactor(usecase): keep only the streaming state the server reads and test the emitted events

## Overview

`SessionRuntime` buffers the in-flight assistant preview in
`StreamingMessage`
(`backend/crates/domain/delta-usecase/src/interactor/session_actor/runtime/streaming.rs`,
lines 20–123): the message id, the thread id, every chunk with its index, and
the final flag. The server never reads that buffer back. `StreamingMessage::text`
and `SessionRuntime::streaming_message` are `cfg(test)`; the Claude hook
handler (`interactor/hooks/on_message_display.rs`) clones the delta into the
buffer and then puts the original on the `AssistantStreaming` event; the
Codex path (`interactor/agent_event.rs`, `stream_assistant_delta` around
line 375) does the same, and the only thing it needs from the state is the
next chunk index for the current message id. Turn end clears the buffer
(`SessionRuntime::apply_turn`). The doc comment on `accumulate_streaming`
says it returns the running text; it returns `()`.

Reduce the state to what production reads. Claude's path needs no buffered
text at all: emit the event straight from the hook. Codex's path needs the
current message id and the count of fragments already emitted for it, so the
next index is assigned and resets when a new message id starts. Keep the
reset on turn end (Stop, interrupt, close) so a new turn starts at index 0.
Drop `chunks`, `thread_id` and `final_` from the state, the `cfg(test)`
accessors, and the comment that says text is returned; rename the type and
methods if the slimmer meaning calls for it. Removing just the test accessors
while keeping the accumulation is not enough; the point is that the server no
longer builds a copy of the text it already forwarded. Confirm with
`git grep` before deleting that nothing else reads the removed fields (for
example a reconnect path that would re-emit the preview); if something does,
keep that part and say so in your report.

Move the tests from the buffer to the contract. The cases in
`interactor/hooks/tests/message_display_streams_and_clears.rs` currently
assert on `streaming_message()` and `text()`; rewrite them to assert on the
emitted `SessionEvent::AssistantStreaming` values — body, thread, index and
final flag per chunk, the thread attribution of a mid-turn branch send, and
that a chunk after a turn end is attributed and indexed as a fresh message.
Out-of-order and duplicate Claude indexes are the browser's concern (the
client accumulates by index); drop the server-side join test and note in the
report where the frontend covers it. Keep the Codex streaming tests and the
fake-Codex e2e expectations on index continuity.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] No `cfg(test)` accessor to the streaming buffer remains in the session
      runtime, and the runtime no longer stores the chunk list (`git grep`
      gates on `session_actor/runtime` appended to `check_command`).
- [x] The `accumulate_streaming` comment no longer claims to return the
      running text (`git grep` gate appended to `check_command`).
- [x] The rewritten hook tests assert the `AssistantStreaming` events a
      Claude `MessageDisplay` sequence produces, including the thread of a
      mid-turn branch send and the reset after Stop and after an interrupt
      (`delta-usecase` tests in `make check`).
- [x] Codex deltas still receive consecutive indexes per message id, starting
      at 0 for a new message and after a turn end (existing Codex streaming
      tests and the fake-Codex e2e suite in `make check`).
- [x] The streaming preview still renders and clears in the fake-Claude e2e
      suite (`make check`).

## Out of scope

- The browser-side accumulation in `frontend/packages`.
- The `AssistantStreaming` wire shape.
