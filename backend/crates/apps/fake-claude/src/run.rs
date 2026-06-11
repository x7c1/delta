//! The engine: wire the launch surfaces together and execute the scenario.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use delta_wire::hooks::{
    PermissionRequestPayload, PreToolUsePayload, SessionStartPayload, StopPayload,
    UserPromptSubmitPayload,
};
use serde_json::{json, Value};

use crate::args::Args;
use crate::hooks::post_json;
use crate::input::{self, InputEvent};
use crate::scenario::{Scenario, SessionStartMode, Step};
use crate::settings::HookEndpoints;
use crate::transcript::TranscriptWriter;

/// Parse the launch, resolve the scenario, and run it to completion.
pub fn run() -> Result<(), String> {
    let args = Args::parse(std::env::args().skip(1));
    let session_id = args
        .effective_session_id()
        .ok_or("launched without --session-id or --resume")?
        .to_owned();
    let settings_path = args
        .settings
        .as_deref()
        .ok_or("launched without --settings")?;
    let settings: Value = serde_json::from_str(
        &std::fs::read_to_string(settings_path)
            .map_err(|e| format!("read settings {settings_path}: {e}"))?,
    )
    .map_err(|e| format!("parse settings {settings_path}: {e}"))?;
    let endpoints = HookEndpoints::from_settings(&settings)?;

    let cwd = std::env::current_dir()
        .map_err(|e| format!("read cwd: {e}"))?
        .to_string_lossy()
        .into_owned();

    let transcript_path = transcript_path_for(&session_id)?;
    let is_resume = args.resume.is_some();
    if is_resume && !transcript_path.exists() {
        // `claude --resume` replays the stored transcript; an unknown id is a
        // startup failure, which the fake mirrors by exiting non-zero.
        return Err(format!(
            "cannot resume {session_id}: no transcript at {}",
            transcript_path.display()
        ));
    }

    let scenario = Scenario::resolve(args.prompt.as_deref())?;

    // A minimal "TUI": the pane shows what this fake is and which session it
    // plays, so an attached human (or a captured pane in CI) can tell what is
    // running. The identifying line comes LAST: a terminal always keeps the
    // cursor row in view, so whatever is printed last stays visible no matter
    // how small the attached client's viewport is — the long transcript path
    // above it may wrap and scroll off. Tests watching for the attach key on
    // this ordering.
    println!("transcript: {}", transcript_path.display());
    println!("fake-claude session {session_id}");

    input::enable_raw_mode();
    let events = input::spawn_reader();

    let mut engine = Engine {
        session_id: session_id.clone(),
        cwd,
        transcript_path: transcript_path.to_string_lossy().into_owned(),
        transcript: TranscriptWriter::open(&transcript_path, &session_id)?,
        endpoints,
        events,
        pending_prompt: args.prompt,
        last_tool_use: None,
        tool_use_seq: 0,
    };

    let source = if is_resume { "resume" } else { "startup" };
    match &scenario.session_start {
        SessionStartMode::Named(mode) if mode == "immediate" => engine.fire_session_start(source),
        SessionStartMode::Named(mode) if mode == "skip" => {}
        SessionStartMode::Named(mode) => {
            return Err(format!("unknown session_start mode: {mode}"));
        }
        SessionStartMode::Delayed { delay_ms } => {
            std::thread::sleep(Duration::from_millis(*delay_ms));
            engine.fire_session_start(source);
        }
    }

    loop {
        for step in &scenario.steps {
            engine.execute(step)?;
        }
        if !scenario.looped {
            break;
        }
    }

    // The script is done but the session is not: a real `claude` sits at its
    // prompt until the pane is killed. Park so tmux does not see an exit (which
    // would end the pane and look like a crashed session).
    eprintln!("fake-claude: scenario complete; idling");
    loop {
        std::thread::park();
    }
}

/// Where this session's transcript lives: `<dir>/<session-id>.jsonl` under
/// `FAKE_CLAUDE_TRANSCRIPT_DIR` (or a fixed temp-dir fallback). Deterministic
/// per session id so a resume finds the transcript the fresh run wrote.
fn transcript_path_for(session_id: &str) -> Result<PathBuf, String> {
    let dir = match std::env::var("FAKE_CLAUDE_TRANSCRIPT_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => std::env::temp_dir().join("fake-claude-transcripts"),
    };
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("create transcript dir {}: {e}", dir.display()))?;
    Ok(dir.join(format!("{session_id}.jsonl")))
}

/// The most recent `tool_use`, kept so `permission_request` and `tool_result`
/// steps can refer to it without restating the call.
struct ToolUse {
    id: String,
    name: String,
    input: Value,
}

struct Engine {
    session_id: String,
    cwd: String,
    transcript_path: String,
    transcript: TranscriptWriter,
    endpoints: HookEndpoints,
    events: Receiver<InputEvent>,
    /// The launch's positional prompt, consumed by the first `await_prompt` —
    /// mirroring how `claude` auto-submits a positional prompt at startup.
    pending_prompt: Option<String>,
    last_tool_use: Option<ToolUse>,
    tool_use_seq: usize,
}

impl Engine {
    fn execute(&mut self, step: &Step) -> Result<(), String> {
        match step {
            Step::AwaitPrompt => {
                let prompt = self.next_prompt()?;
                // Hook first, then the transcript line: the real `claude` fires
                // `UserPromptSubmit` before the user line lands in the JSONL
                // (the server is built to tolerate — and expects — that order).
                self.fire(
                    "UserPromptSubmit",
                    &self.endpoints.user_prompt_submit,
                    &UserPromptSubmitPayload {
                        prompt: prompt.clone(),
                        session_id: self.session_id.clone(),
                        transcript_path: self.transcript_path.clone(),
                        cwd: self.cwd.clone(),
                    },
                );
                self.transcript.user_text(&prompt)
            }
            Step::Reply { text, thinking } => {
                let mut blocks = Vec::new();
                if let Some(thinking) = thinking {
                    blocks.push(json!({ "type": "thinking", "thinking": thinking }));
                }
                blocks.push(json!({ "type": "text", "text": text }));
                self.transcript.assistant_blocks(blocks)
            }
            Step::ToolUse { name, input } => {
                let id = format!("toolu_fake_{:04}", self.tool_use_seq);
                self.tool_use_seq += 1;
                self.transcript.assistant_blocks(vec![json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": input,
                })])?;
                self.fire(
                    "PreToolUse",
                    &self.endpoints.pre_tool_use,
                    &PreToolUsePayload {
                        session_id: self.session_id.clone(),
                        tool_name: name.clone(),
                        tool_input: input.clone(),
                        tool_use_id: id.clone(),
                    },
                );
                self.last_tool_use = Some(ToolUse {
                    id,
                    name: name.clone(),
                    input: input.clone(),
                });
                Ok(())
            }
            Step::PermissionRequest => {
                let tool_use = self
                    .last_tool_use
                    .as_ref()
                    .ok_or("permission_request step without a preceding tool_use")?;
                let payload = PermissionRequestPayload {
                    session_id: self.session_id.clone(),
                    tool_name: tool_use.name.clone(),
                    tool_input: tool_use.input.clone(),
                };
                self.fire(
                    "PermissionRequest",
                    &self.endpoints.permission_request,
                    &payload,
                );
                Ok(())
            }
            Step::ToolResult { is_error } => {
                let id = self
                    .last_tool_use
                    .as_ref()
                    .map(|t| t.id.clone())
                    .ok_or("tool_result step without a preceding tool_use")?;
                self.transcript.tool_result(&id, *is_error)
            }
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
                self.await_interrupt()?;
                // The marker line — and deliberately NO `Stop` hook, exactly
                // like a real interrupt: the transcript tail is what tells the
                // server the turn was aborted.
                self.transcript.interrupt_marker()
            }
            Step::WriteQueuedCommand { text } => self.transcript.queued_command(text),
            Step::Delay { ms } => {
                std::thread::sleep(Duration::from_millis(*ms));
                Ok(())
            }
            Step::Hang => loop {
                std::thread::park();
            },
        }
    }

    /// The next submitted prompt: the launch's positional prompt first, then
    /// whatever the pane input submits. Escapes pressed while idle are ignored
    /// (there is no turn to interrupt), like a TUI at its prompt.
    fn next_prompt(&mut self) -> Result<String, String> {
        if let Some(prompt) = self.pending_prompt.take() {
            return Ok(prompt);
        }
        loop {
            match self.events.recv() {
                Ok(InputEvent::Prompt(text)) => return Ok(text),
                Ok(InputEvent::Interrupt) => continue,
                Err(_) => return Err("stdin closed while awaiting a prompt".to_owned()),
            }
        }
    }

    /// Block until Escape arrives. Prompts submitted while a turn is in
    /// flight are dropped: modelling Claude's queued-command behaviour is the
    /// explicit `write_queued_command` step's job, not an implicit side
    /// effect of waiting.
    fn await_interrupt(&mut self) -> Result<(), String> {
        loop {
            match self.events.recv() {
                Ok(InputEvent::Interrupt) => return Ok(()),
                Ok(InputEvent::Prompt(dropped)) => {
                    eprintln!("fake-claude: dropping prompt submitted mid-turn: {dropped}");
                }
                Err(_) => return Err("stdin closed while awaiting an interrupt".to_owned()),
            }
        }
    }

    fn fire_session_start(&self, source: &str) {
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
    /// scenario's later assertions will surface the breakage.
    fn fire<P: serde::Serialize>(&self, event: &str, url: &str, payload: &P) {
        match post_json(url, payload) {
            Ok(status) if (200..300).contains(&status) => {}
            Ok(status) => eprintln!("fake-claude: {event} hook returned HTTP {status}"),
            Err(err) => eprintln!("fake-claude: {event} hook failed: {err}"),
        }
    }
}
