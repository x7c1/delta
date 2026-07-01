//! `fake-claude`: a scripted stand-in for the `claude` binary.
//!
//! Delta launches `claude` inside a tmux pane and observes it exclusively
//! through three external surfaces:
//!
//! 1. the CLI flags it was launched with (`--settings <file>`,
//!    `--session-id <uuid>`, `--resume <id>`, plus an optional positional
//!    first prompt),
//! 2. the HTTP hooks configured in the settings file (`SessionStart`,
//!    `UserPromptSubmit`, `Stop`, `PreToolUse`, `PermissionRequest`,
//!    `SessionEnd`), and
//! 3. the JSONL transcript whose path the hook payloads report.
//!
//! This binary speaks exactly those three surfaces — and nothing else — so a
//! server pointed at it (via `DELTA_CLAUDE_BIN`) runs its real spawn → tmux →
//! hooks → transcript → tail loop end to end, while the "model" follows a
//! deterministic scenario script instead of an LLM. See [`scenario`] for the
//! step vocabulary and how a scenario file is selected.
//!
//! Input arrives the way tmux delivers it: raw bytes on stdin (`send-keys`),
//! where a line of text followed by Enter is a prompt submission and a lone
//! Escape byte is an interrupt. See [`input`].

mod args;
mod hooks;
mod input;
mod run;
mod scenario;
mod settings;
mod transcript;

use std::process::ExitCode;

fn main() -> ExitCode {
    match run::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            // The pane is the only console this program has; a launch problem
            // (missing settings, unreadable scenario) must be visible there.
            eprintln!("fake-claude: {message}");
            ExitCode::FAILURE
        }
    }
}
