//! Rendering the Claude Code session settings JSON.
//!
//! The session needs hooks pointing back at this server so Delta receives
//! `UserPromptSubmit`, `Stop`, `PreToolUse`, `PermissionRequest`, `SessionStart`,
//! and `SessionEnd` callbacks. Most are native `http` hooks; `SessionStart` is
//! the exception — Claude Code does NOT deliver `SessionStart` to `http` hooks
//! (verified empirically: every other event arrives over http, `SessionStart`
//! never does), so it is rendered as a `command` hook that `curl`s the same
//! server endpoint. The server
//! renders these settings itself (rather than copying a static template) so the
//! hook URLs always match the port the server is actually listening on — there
//! is no second source of truth to drift out of sync. The rendered JSON is
//! written to a Delta-owned file and handed to `claude --settings <file>`, so it
//! never has to be written into (and risk clobbering) the session's working
//! directory.

use serde_json::json;

/// Render the session settings JSON for hooks pointing at `127.0.0.1:<port>`.
pub fn render_session_settings(port: u16) -> String {
    let url = |path: &str| format!("http://127.0.0.1:{port}/hooks/{path}");
    let http_hook = |path: &str| {
        json!({
            "hooks": [
                { "type": "http", "url": url(path), "timeout": 30 }
            ]
        })
    };
    // `SessionStart` is delivered only to `command` hooks (Claude Code does not
    // POST it to `http` hooks), so this one forwards the hook's stdin JSON to the
    // same server endpoint with `curl`. `-o /dev/null` discards the server's
    // response so it is never fed back to Claude as hook output; `-m 5` bounds
    // startup if the server is somehow unreachable; the `content-type` header
    // lets the server's JSON extractor parse the body.
    let command_post_hook = |path: &str| {
        json!({
            "hooks": [
                {
                    "type": "command",
                    "command": format!(
                        "curl -sS -m 5 -o /dev/null -X POST {} \
                         -H 'content-type: application/json' --data-binary @-",
                        url(path)
                    )
                }
            ]
        })
    };
    let settings = json!({
        // Force the dark theme: the embedded terminal renders on a dark
        // background (see the web terminal's xterm theme), so Claude must emit
        // dark-appropriate colors to stay readable — regardless of whatever
        // theme the user has set globally. Passed via `--settings`, so this only
        // applies to Delta's sessions and never touches the user's own config.
        "theme": "dark",
        "hooks": {
            "UserPromptSubmit": [http_hook("user-prompt-submit")],
            "Stop": [http_hook("stop")],
            "PreToolUse": [{
                "matcher": "*",
                "hooks": [
                    { "type": "http", "url": url("pre-tool-use"), "timeout": 30 }
                ]
            }],
            // PermissionRequest fires only when an interactive permission dialog
            // actually appears (not for auto/classifier-approved calls), so it is
            // the signal for "a human answer is genuinely pending". PreToolUse
            // fires for every call and only records the request.
            "PermissionRequest": [http_hook("permission-request")],
            // SessionStart fires when a session's TUI is ready to accept input
            // (source=startup on a fresh launch, source=resume after
            // `claude --resume`). It is Delta's launch-readiness signal: it binds
            // a fresh spawn immediately (even a prompt-less one) and releases a
            // resumed session's held first prompt once the cold pane can accept
            // it — replacing the old fixed post-launch settle. It MUST be a
            // `command` hook (curl): Claude Code does not deliver SessionStart to
            // `http` hooks, so an http hook here would never fire and a resumed
            // session's held first prompt would never be released.
            "SessionStart": [command_post_hook("session-start")],
            // SessionEnd fires when a session terminates. It is the precise early
            // failure signal for a launch (fresh spawn or resume) that exited
            // before it became ready: that launch failed, so Delta reaps it
            // immediately instead of waiting out the watchdog deadline.
            "SessionEnd": [http_hook("session-end")],
        }
    });
    // Pretty-printed so the on-disk file is human-readable when inspected.
    // Rendering never fails for this fixed shape.
    serde_json::to_string_pretty(&settings).expect("session settings serialize")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_the_port_in_every_hook_url() {
        let rendered = render_session_settings(9999);
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();

        // The embedded terminal is dark, so the session's theme is forced dark.
        assert_eq!(parsed["theme"], "dark");

        assert_eq!(
            parsed["hooks"]["UserPromptSubmit"][0]["hooks"][0]["url"],
            "http://127.0.0.1:9999/hooks/user-prompt-submit"
        );
        assert_eq!(
            parsed["hooks"]["Stop"][0]["hooks"][0]["url"],
            "http://127.0.0.1:9999/hooks/stop"
        );
        assert_eq!(
            parsed["hooks"]["PreToolUse"][0]["hooks"][0]["url"],
            "http://127.0.0.1:9999/hooks/pre-tool-use"
        );
        assert_eq!(parsed["hooks"]["PreToolUse"][0]["matcher"], "*");
        assert_eq!(
            parsed["hooks"]["PermissionRequest"][0]["hooks"][0]["url"],
            "http://127.0.0.1:9999/hooks/permission-request"
        );
        // SessionStart is a `command` hook (Claude Code does not deliver it over
        // http), curling the same server endpoint and forwarding the stdin
        // payload. The server's response is discarded so it is not fed back to
        // Claude as hook output.
        assert_eq!(
            parsed["hooks"]["SessionStart"][0]["hooks"][0]["type"],
            "command"
        );
        let session_start_command = parsed["hooks"]["SessionStart"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(session_start_command.contains("http://127.0.0.1:9999/hooks/session-start"));
        assert!(session_start_command.contains("--data-binary @-"));
        assert!(session_start_command.contains("-o /dev/null"));

        assert_eq!(
            parsed["hooks"]["SessionEnd"][0]["hooks"][0]["url"],
            "http://127.0.0.1:9999/hooks/session-end"
        );
    }
}
