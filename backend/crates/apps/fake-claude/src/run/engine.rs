//! The per-step driver: the [`Engine`] that executes one scenario step at a
//! time against the transcript, the hook endpoints, and the pane input. The
//! tool-call steps live in `tool_steps`.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use delta_wire::hooks::{
    MessageDisplayPayload, SessionStartPayload, StopPayload, UserPromptSubmitPayload,
};
use serde_json::{json, Value};

use crate::hooks::post_json;
use crate::input::{InputEvent, Submission};
use crate::pasted_content;
use crate::scenario::Step;
use crate::settings::HookEndpoints;
use crate::transcript::TranscriptWriter;

use super::tool_steps::ToolUse;

/// The running session's state: what the steps read and write as the
/// scenario advances.
pub(super) struct Engine {
    pub(super) session_id: String,
    pub(super) cwd: String,
    pub(super) transcript_path: String,
    pub(super) transcript: TranscriptWriter,
    pub(super) endpoints: HookEndpoints,
    pub(super) events: Receiver<InputEvent>,
    /// The launch's positional prompt, consumed by the first `await_prompt` —
    /// mirroring how `claude` auto-submits a positional prompt at startup.
    pub(super) pending_prompt: Option<String>,
    /// The `<pasted_content>` block id when the scenario wraps long pastes
    /// (`wrap_pastes`), `None` when every prompt is submitted bare.
    pub(super) paste_wrap_id: Option<String>,
    /// Prompts the scenario enqueued mid-turn (`enqueue_prompt`), awaiting
    /// their `dequeue_prompt` replay — mirroring claude's prompt queue.
    pub(super) queued_prompts: VecDeque<String>,
    pub(super) last_tool_use: Option<ToolUse>,
    pub(super) tool_use_seq: usize,
    /// A fresh display-message id per `stream_text` step, mirroring how the real
    /// `claude` stamps one `message_id` across a streamed message's chunks.
    pub(super) message_id_seq: usize,
    /// The `additionalContext` the most recent `UserPromptSubmit` hook response
    /// injected (empty when none): the real `claude` folds it into the model
    /// prompt, so the fake records it and exposes it to `reply` steps via the
    /// `{additional_context}` placeholder — letting a test observe, end to end,
    /// exactly what context the server delivered.
    pub(super) last_additional_context: String,
}

impl Engine {
    pub(super) fn execute(&mut self, step: &Step) -> Result<(), String> {
        match step {
            Step::AwaitPrompt => {
                let submission = self.next_prompt()?;
                // What Claude Code submits: the entered text, with a long
                // paste wrapped when the scenario opts into the wrapper.
                let prompt = match &self.paste_wrap_id {
                    Some(id) => pasted_content::wrap_submission(&submission, id),
                    None => submission.text(),
                };
                self.submit_prompt(&prompt, false)
            }
            Step::Reply { text, thinking } => {
                // `{additional_context}` substitutes the context the most
                // recent `UserPromptSubmit` response injected, so a scenario
                // can surface it in the visible conversation for assertions.
                let text = text.replace("{additional_context}", &self.last_additional_context);
                let mut blocks = Vec::new();
                if let Some(thinking) = thinking {
                    blocks.push(json!({ "type": "thinking", "thinking": thinking }));
                }
                blocks.push(json!({ "type": "text", "text": text }));
                self.transcript.assistant_blocks(blocks)
            }
            Step::StreamText { deltas } => {
                // Stream the visible assistant text live via `MessageDisplay`,
                // before any transcript line lands — exactly the order the real
                // `claude` delivers it. One `message_id` spans the message; the
                // chunks carry increasing `index` and only the last is `final`.
                let message_id = format!("msg_fake_{:04}", self.message_id_seq);
                self.message_id_seq += 1;
                let last = deltas.len().saturating_sub(1);
                for (index, delta) in deltas.iter().enumerate() {
                    self.fire(
                        "MessageDisplay",
                        &self.endpoints.message_display,
                        &MessageDisplayPayload {
                            session_id: self.session_id.clone(),
                            message_id: message_id.clone(),
                            index: index as u32,
                            r#final: index == last,
                            delta: delta.clone(),
                            turn_id: Some(message_id.clone()),
                        },
                    );
                }
                Ok(())
            }
            Step::ToolUse { name, input } => self.tool_use(name, input),
            Step::PostToolUse => self.post_tool_use(),
            Step::PermissionRequest { on_allow, on_deny } => {
                self.permission_request(on_allow, on_deny)
            }
            Step::ToolResult { is_error } => self.tool_result(*is_error),
            Step::TaskNotification { drop_tool_use_id } => {
                self.task_notification(*drop_tool_use_id)
            }
            Step::TaskOutput { status } => self.task_output(status),
            Step::Stop { stop_reason } => {
                self.fire(
                    "Stop",
                    &self.endpoints.stop,
                    &StopPayload {
                        session_id: self.session_id.clone(),
                        stop_reason: stop_reason.clone(),
                    },
                );
                Ok(())
            }
            Step::AwaitInterrupt => {
                self.await_escape()?;
                // The marker line — and deliberately NO `Stop` hook, exactly
                // like a real interrupt: the transcript tail is what tells the
                // server the turn was aborted.
                self.transcript.interrupt_marker()
            }
            Step::AwaitEscape => {
                // Block until Escape, writing nothing: the cancel's effect (an
                // `is_error` tool_result) is the scenario's next step. This
                // models cancelling an AskUserQuestion, where a single Escape
                // cancels the call and the TUI then writes the error result.
                self.await_escape()
            }
            Step::EnqueuePrompt { text } => {
                // A prompt submitted while the turn is busy: claude records
                // only the uuid-less `queue-operation` enqueue line now (no
                // hook fires) and replays the prompt at dequeue.
                self.queued_prompts.push_back(text.clone());
                self.transcript.queue_operation_enqueue(text)
            }
            Step::DequeuePrompt => {
                let prompt = self
                    .queued_prompts
                    .pop_front()
                    .ok_or("dequeue_prompt step without a pending enqueue_prompt")?;
                // The dequeued prompt flows the same path as a TUI-typed one:
                // its own `UserPromptSubmit`, then a plain user line (stamped
                // `promptSource: "queued"`).
                self.submit_prompt(&prompt, true)
            }
            Step::Delay { ms } => {
                std::thread::sleep(Duration::from_millis(*ms));
                Ok(())
            }
            Step::Hang => loop {
                std::thread::park();
            },
            Step::SwallowPrompt => {
                // Consume the prompt off stdin without firing
                // `UserPromptSubmit` or writing anything (see variant doc).
                let _swallowed = self.next_prompt()?;
                Ok(())
            }
            Step::LocalCommand { opens_dialog } => {
                // The command runs without firing any hook or writing
                // anything (see the variant doc).
                let _command = self.next_prompt()?;
                if *opens_dialog {
                    // The dialog it left up swallows whatever is typed until
                    // an Escape dismisses it.
                    self.await_escape()?;
                }
                Ok(())
            }
            Step::CompactGroup => self.transcript.compact_group(),
            Step::Relocate { dir } => self.relocate(dir),
        }
    }

    /// The submit sequence a fresh prompt and a dequeued replay share: fire
    /// `UserPromptSubmit` first, then write the user transcript line — the
    /// real `claude` fires the hook before the line lands in the JSONL (the
    /// server is built to tolerate — and expects — that order). Records any
    /// injected `additionalContext` from the hook response, like the real
    /// `claude` consuming it; exposed to `reply` steps via the
    /// `{additional_context}` placeholder.
    fn submit_prompt(&mut self, prompt: &str, dequeued: bool) -> Result<(), String> {
        let body = self.fire(
            "UserPromptSubmit",
            &self.endpoints.user_prompt_submit,
            &UserPromptSubmitPayload {
                prompt: prompt.to_owned(),
                session_id: self.session_id.clone(),
                transcript_path: self.transcript_path.clone(),
                cwd: self.cwd.clone(),
            },
        );
        self.last_additional_context = body
            .as_deref()
            .and_then(|b| serde_json::from_str::<Value>(b).ok())
            .and_then(|v| {
                v.pointer("/hookSpecificOutput/additionalContext")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        if dequeued {
            self.transcript.dequeued_user_text(prompt)
        } else {
            self.transcript.user_text(prompt)
        }
    }

    /// Enter a worktree the way Claude Code does: a new working directory, and
    /// the transcript moved to that directory's project directory (beside the
    /// current one, under the same transcript root). Later hooks report both.
    fn relocate(&mut self, dir: &str) -> Result<(), String> {
        let cwd = Path::new(&self.cwd).join(dir);
        std::fs::create_dir_all(&cwd)
            .map_err(|e| format!("create worktree dir {}: {e}", cwd.display()))?;
        let cwd = cwd.to_string_lossy().into_owned();
        let old_path = PathBuf::from(&self.transcript_path);
        let root = old_path
            .parent()
            .ok_or("transcript path has no parent directory")?;
        let new_path = root.join(dir).join(format!("{}.jsonl", self.session_id));
        self.transcript.relocate(&new_path, &cwd)?;
        eprintln!(
            "fake-claude: relocated transcript {} -> {}",
            old_path.display(),
            new_path.display()
        );
        self.transcript_path = new_path.to_string_lossy().into_owned();
        self.cwd = cwd;
        Ok(())
    }

    /// The next submitted prompt: the launch's positional prompt first, then
    /// whatever the pane input submits. Escapes pressed while idle are ignored
    /// (there is no turn to interrupt), like a TUI at its prompt.
    /// The positional prompt never went through the pane, so it counts as
    /// typed.
    fn next_prompt(&mut self) -> Result<Submission, String> {
        if let Some(prompt) = self.pending_prompt.take() {
            return Ok(Submission::typed(&prompt));
        }
        loop {
            match self.events.recv() {
                Ok(InputEvent::Prompt(submission)) => return Ok(submission),
                Ok(InputEvent::Interrupt) => continue,
                Err(_) => return Err("stdin closed while awaiting a prompt".to_owned()),
            }
        }
    }

    /// Block until Escape arrives. Prompts submitted while a turn is in
    /// flight are dropped: modelling claude's prompt queue is the explicit
    /// `enqueue_prompt`/`dequeue_prompt` steps' job, not an implicit side
    /// effect of waiting.
    ///
    /// Shared by the `await_interrupt` (turn interrupt) and `await_escape`
    /// (AskUserQuestion cancel) steps — both wait for the same Escape byte and
    /// differ only in what they write afterwards.
    fn await_escape(&mut self) -> Result<(), String> {
        loop {
            match self.events.recv() {
                Ok(InputEvent::Interrupt) => return Ok(()),
                Ok(InputEvent::Prompt(dropped)) => {
                    eprintln!(
                        "fake-claude: dropping prompt submitted mid-turn: {}",
                        dropped.text()
                    );
                }
                Err(_) => return Err("stdin closed while awaiting an escape".to_owned()),
            }
        }
    }

    pub(super) fn fire_session_start(&self, source: &str) {
        self.fire(
            "SessionStart",
            &self.endpoints.session_start,
            &SessionStartPayload {
                session_id: self.session_id.clone(),
                source: source.to_owned(),
                cwd: self.cwd.clone(),
                transcript_path: self.transcript_path.clone(),
            },
        );
    }

    /// Fire one hook, logging (but tolerating) delivery failures — a real
    /// `claude` keeps running when a hook endpoint misbehaves, and the
    /// scenario's later assertions will surface the breakage. Returns the
    /// response body on a 2xx (the hooks whose response `claude` consumes need
    /// it), `None` otherwise.
    pub(super) fn fire<P: serde::Serialize>(
        &self,
        event: &str,
        url: &str,
        payload: &P,
    ) -> Option<String> {
        match post_json(url, payload) {
            Ok((status, body)) if (200..300).contains(&status) => Some(body),
            Ok((status, _)) => {
                eprintln!("fake-claude: {event} hook returned HTTP {status}");
                None
            }
            Err(err) => {
                eprintln!("fake-claude: {event} hook failed: {err}");
                None
            }
        }
    }
}
