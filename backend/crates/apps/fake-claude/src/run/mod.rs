//! Wiring the launch surfaces together and executing the scenario.
//!
//! This module parses the launch and drives the scenario; `engine` executes
//! each step, and `tool_steps` holds the tool-call steps.

mod engine;
mod tool_steps;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use crate::args::Args;
use crate::input;
use crate::pasted_content;
use crate::scenario::{Scenario, SessionStartMode};
use crate::settings::HookEndpoints;
use crate::transcript::TranscriptWriter;

use engine::Engine;

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
        cwd: cwd.clone(),
        transcript_path: transcript_path.to_string_lossy().into_owned(),
        transcript: TranscriptWriter::open(&transcript_path, &session_id, &cwd)?,
        endpoints,
        events,
        pending_prompt: args.prompt,
        paste_wrap_id: scenario
            .wrap_pastes
            .then(|| pasted_content::session_paste_id(&session_id)),
        queued_prompts: VecDeque::new(),
        last_tool_use: None,
        tool_use_seq: 0,
        message_id_seq: 0,
        last_additional_context: String::new(),
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
