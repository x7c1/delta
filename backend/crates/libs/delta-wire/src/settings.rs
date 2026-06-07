//! Rendering the Claude Code session's `.claude/settings.json`.
//!
//! The session needs native HTTP hooks pointing back at this server so Delta
//! receives `UserPromptSubmit`, `Stop`, and `PreToolUse` callbacks. The server
//! renders these settings itself (rather than copying a static template) so the
//! hook URLs always match the port the server is actually listening on — there
//! is no second source of truth to drift out of sync.

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
        "hooks": {
            "UserPromptSubmit": [http_hook("user-prompt-submit")],
            "Stop": [http_hook("stop")],
            "PreToolUse": [{
                "matcher": "*",
                "hooks": [
                    { "type": "http", "url": url("pre-tool-use"), "timeout": 30 }
                ]
            }],
        }
    });
    // Pretty-printed so the on-disk file is human-readable when inspecting a
    // session workdir. Rendering never fails for this fixed shape.
    serde_json::to_string_pretty(&settings).expect("session settings serialize")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_the_port_in_every_hook_url() {
        let rendered = render_session_settings(9999);
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();

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
    }
}
