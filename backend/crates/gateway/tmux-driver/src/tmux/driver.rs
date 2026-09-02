//! The [`TmuxDriver`] surface: each trait method turns its arguments into the
//! argv vectors `commands` builds and runs them against Delta's tmux server.

use async_trait::async_trait;

use delta_usecase::TmuxDriver;

use super::commands::{
    clear_input_commands, input_commands, key_command, new_session_args, submit_command,
    KEY_SETTLE, SUBMIT_ENTER_DELAY,
};
use super::Tmux;

#[async_trait]
impl TmuxDriver for Tmux {
    async fn has_session(&self, name: &str) -> std::result::Result<bool, delta_usecase::Error> {
        // `tmux has-session` exits 0 when the session exists and non-zero when it
        // does not (or the server is not running). A non-zero exit here is the
        // expected "absent" signal, not an error to propagate.
        let output = self
            .output(&["has-session", "-t", name])
            .await
            .map_err(delta_usecase::Error::from)?;
        Ok(output.status.success())
    }

    async fn create_session(
        &self,
        name: &str,
        workdir: &str,
        command: &[String],
    ) -> std::result::Result<(), delta_usecase::Error> {
        // Write Delta's fixed config before the server-starting `new-session`
        // runs. `-f <conf_path>` (added by `output`) is read only when the server
        // boots, and `new-session` is the call that boots it (`has-session` and
        // friends just fail when no server is running). Writing on each create is
        // idempotent: once the server is up the file is left untouched, and a
        // rewrite for an already-running server is a harmless no-op. This is what
        // makes the embedded pane (terminal type, focus events, status bar) the
        // same on every machine — see DELTA_TMUX_CONF. The write is hardened
        // (0600, no symlink following) because tmux executes what it reads —
        // see `write_conf`.
        self.write_conf()
            .await
            .map_err(delta_usecase::Error::from)?;

        let args = new_session_args(name, workdir, command);
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run(&borrowed)
            .await
            .map_err(delta_usecase::Error::from)?;

        // No post-launch settle delay. Readiness is now event-driven via the
        // `SessionStart` hook, which fires when the TUI can actually accept
        // input, so there is no keystroke to race against `new-session`'s return:
        // a fresh spawn submits its first prompt as a launch positional argument
        // (the server never types into a cold pane), and a resume holds its first
        // keystroke until `SessionStart(source=resume)` arrives — measured ~2s
        // after launch, far past the 750ms a fixed settle could safely wait. The
        // 250ms `SUBMIT_ENTER_DELAY` in `send_line` is unrelated (it spaces the
        // submit Enter past Claude's paste-burst window) and stays.
        Ok(())
    }

    async fn send_line(
        &self,
        pane: &str,
        text: &str,
    ) -> std::result::Result<(), delta_usecase::Error> {
        // Type the message (clear + literal text) without submitting it.
        for args in input_commands(pane, text) {
            let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
            self.run(&borrowed)
                .await
                .map_err(delta_usecase::Error::from)?;
        }
        // Wait out Claude's paste-burst window before the submit Enter, so the
        // Enter lands as a discrete keystroke and is not absorbed into the
        // just-typed text (see SUBMIT_ENTER_DELAY).
        tokio::time::sleep(SUBMIT_ENTER_DELAY).await;
        let submit = submit_command(pane);
        let borrowed: Vec<&str> = submit.iter().map(String::as_str).collect();
        self.run(&borrowed)
            .await
            .map_err(delta_usecase::Error::from)
    }

    async fn send_keys(
        &self,
        pane: &str,
        keys: &[&str],
    ) -> std::result::Result<(), delta_usecase::Error> {
        // Each key is sent as its own discrete `send-keys` invocation with a
        // small settle in between, so the TUI processes one navigation/toggle
        // keystroke at a time. Batching them into one `send-keys` call risks the
        // widget coalescing rapid keys (e.g. a Down+Enter racing the highlight
        // move), which a deliberate human cadence avoids; the settle restores
        // that cadence. The keys come from the pinned key-sequence generator, so
        // they are a fixed vocabulary (`Down`, `Up`, `Space`, `Enter`, …) and
        // never literal text.
        for key in keys {
            let args = key_command(pane, key);
            let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
            self.run(&borrowed)
                .await
                .map_err(delta_usecase::Error::from)?;
            tokio::time::sleep(KEY_SETTLE).await;
        }
        Ok(())
    }

    async fn clear_input(&self, pane: &str) -> std::result::Result<(), delta_usecase::Error> {
        let args = clear_input_commands(pane);
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run(&borrowed)
            .await
            .map_err(delta_usecase::Error::from)
    }

    async fn kill_session(&self, name: &str) -> std::result::Result<(), delta_usecase::Error> {
        self.run(&["kill-session", "-t", name])
            .await
            .map_err(delta_usecase::Error::from)
    }
}
