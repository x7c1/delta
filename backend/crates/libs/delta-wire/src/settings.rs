//! Rendering the Claude Code session settings JSON.
//!
//! The session needs native HTTP hooks pointing back at this server so Delta
//! receives `UserPromptSubmit`, `Stop`, `PreToolUse`, `PermissionRequest`, and
//! `SessionEnd` callbacks. The server
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
            // SessionEnd fires when a session terminates. It is the precise early
            // failure signal for a fresh spawn that exited before its first
            // UserPromptSubmit ever bound it: that launch failed, so Delta reaps
            // it immediately instead of waiting out the watchdog deadline.
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
        assert_eq!(
            parsed["hooks"]["SessionEnd"][0]["hooks"][0]["url"],
            "http://127.0.0.1:9999/hooks/session-end"
        );
    }
}
