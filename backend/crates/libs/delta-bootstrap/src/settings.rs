//! Rendering the Claude Code session settings JSON.
//!
//! The session needs hooks pointing back at this server so Delta receives
//! `UserPromptSubmit`, `Stop`, `MessageDisplay`, `PreToolUse`, `PostToolUse`,
//! `PermissionRequest`, `SessionStart`, and `SessionEnd` callbacks. Most are
//! native `http` hooks; `SessionStart` is
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
    // The shared `curl` invocation for every entry that forwards its stdin JSON
    // to the server (the `SessionStart` command hook and the `statusLine`
    // command both use it). `-o /dev/null` discards the server's response so it
    // is never fed back to Claude — for a hook that means no hook output, for
    // `statusLine` an empty terminal status line; `-m 5` bounds the call if the
    // server is somehow unreachable; the `content-type` header lets the
    // server's JSON extractor parse the body; `--data-binary @-` forwards
    // stdin verbatim.
    let curl_post = |path: &str| {
        format!(
            "curl -sS -m 5 -o /dev/null -X POST {} \
             -H 'content-type: application/json' --data-binary @-",
            url(path)
        )
    };
    // `SessionStart` is delivered only to `command` hooks (Claude Code does not
    // POST it to `http` hooks), so this one forwards the hook's stdin JSON to the
    // same server endpoint with `curl`.
    let command_post_hook = |path: &str| {
        json!({
            "hooks": [
                {
                    "type": "command",
                    "command": curl_post(path)
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
        // Claude Code pipes a JSON snapshot of session state (selected model,
        // context-window usage, rate limits, cost, workspace) to this command's
        // stdin on every status-line refresh. None of that is in the transcript
        // JSONL, so this is how the server learns it: the command `curl`s the
        // stdin payload to the server, which rebroadcasts it to the browser.
        // Claude renders the command's STDOUT as the status-line text in the
        // (delta-embedded) terminal; delta shows this data in the web UI
        // instead, so the shared `curl_post`'s `-o /dev/null` makes the command
        // emit nothing — both discarding the server's response and leaving the
        // terminal status line empty.
        "statusLine": {
            "type": "command",
            "command": curl_post("status-line")
        },
        "hooks": {
            "UserPromptSubmit": [http_hook("user-prompt-submit")],
            "Stop": [http_hook("stop")],
            // MessageDisplay fires repeatedly while a response is being
            // generated (before the transcript is flushed), delivering each
            // visible assistant text chunk. Delta uses it to live-stream the
            // in-flight reply into the conversation pane. It is delivered to
            // http hooks like every event except SessionStart, so a plain http
            // hook is enough; the handler stays passive (empty 200).
            "MessageDisplay": [http_hook("message-display")],
            "PreToolUse": [{
                "matcher": "*",
                "hooks": [
                    { "type": "http", "url": url("pre-tool-use"), "timeout": 30 }
                ]
            }],
            // PostToolUse fires when a tool call completes. Delta acts on it
            // only for the subagent (`Agent`/`Task`) case: it closes a
            // FOREGROUND subagent's running indicator, and for a background
            // launch (which returns at once) it records the `agentId` that the
            // later completion notification is matched by. Every other tool's
            // PostToolUse is an empty 200.
            "PostToolUse": [{
                "matcher": "*",
                "hooks": [
                    { "type": "http", "url": url("post-tool-use"), "timeout": 30 }
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
            parsed["hooks"]["MessageDisplay"][0]["hooks"][0]["url"],
            "http://127.0.0.1:9999/hooks/message-display"
        );
        assert_eq!(
            parsed["hooks"]["PreToolUse"][0]["hooks"][0]["url"],
            "http://127.0.0.1:9999/hooks/pre-tool-use"
        );
        assert_eq!(parsed["hooks"]["PreToolUse"][0]["matcher"], "*");
        assert_eq!(
            parsed["hooks"]["PostToolUse"][0]["hooks"][0]["url"],
            "http://127.0.0.1:9999/hooks/post-tool-use"
        );
        assert_eq!(parsed["hooks"]["PostToolUse"][0]["matcher"], "*");
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

    #[test]
    fn emits_a_status_line_command_targeting_the_same_port() {
        let rendered = render_session_settings(9999);
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();

        // `statusLine` is a top-level `command` entry (not a hook): Claude Code
        // pipes the status JSON to its stdin and renders its stdout as the
        // status line. It must target `/hooks/status-line` on the same port as
        // the hooks, forward stdin (`--data-binary @-`), and emit nothing to
        // stdout (`-o /dev/null`) so the embedded terminal's status line stays
        // empty.
        assert_eq!(parsed["statusLine"]["type"], "command");
        let command = parsed["statusLine"]["command"].as_str().unwrap();
        assert!(command.contains("http://127.0.0.1:9999/hooks/status-line"));
        assert!(command.contains("--data-binary @-"));
        assert!(command.contains("-o /dev/null"));
    }
}
