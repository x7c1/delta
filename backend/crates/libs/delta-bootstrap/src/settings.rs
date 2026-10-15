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

use std::time::Duration;

use serde_json::json;

/// The timeout, in seconds, Claude Code is given for every HTTP hook except
/// `PermissionRequest`. These hooks answer at once, so the bound only matters
/// when the server is unreachable.
const HOOK_TIMEOUT_SECS: u64 = 30;

/// How much longer than Delta's decision wait Claude Code waits on the
/// `PermissionRequest` hook.
const PERMISSION_HOOK_TIMEOUT_MARGIN: Duration = Duration::from_secs(20);

/// The timeout, in seconds, Claude Code is given for the `PermissionRequest`
/// hook when Delta waits `decision_deadline` for the browser's decision.
///
/// The hook response blocks for that whole wait, so the timeout must outlast
/// it: with a shorter one Claude Code would abandon the hook mid-wait and show
/// its own prompt while the browser card still offers Allow and Deny, and an
/// answer given on the card would land on nothing. With the margin, the
/// fallback to the terminal prompt is always Delta's deliberate empty
/// passthrough at the end of its own wait. Derived from the configured wait
/// rather than fixed, so a deadline stretched through the environment keeps
/// the same guarantee.
fn permission_hook_timeout_secs(decision_deadline: Duration) -> u64 {
    // Round a fractional deadline up: the timeout is whole seconds.
    let wait_secs = decision_deadline.as_secs() + u64::from(decision_deadline.subsec_nanos() > 0);
    wait_secs + PERMISSION_HOOK_TIMEOUT_MARGIN.as_secs()
}

/// Render the session settings JSON for hooks pointing at `127.0.0.1:<port>`.
///
/// `hook_secret` is the secret carried back to the server as an `hs`
/// query parameter on every hook URL, so the hook auth guard (see
/// `delta-server`'s `hook_auth_guard`) can tell a genuine Claude Code callback
/// apart from a forged local POST. It rides in the URL — not a header — because
/// Claude Code's native `http` hooks have no proven way to set request headers
/// here, and the URL form works uniformly for both the `http` hooks and the
/// `command` (`curl`) hook. The secret is hex text (URL-safe), so it needs no
/// percent-encoding.
///
/// `permission_decision_deadline` is how long the server holds a
/// `PermissionRequest` hook response waiting for the browser's decision; the
/// hook's timeout is that wait plus a margin, so Claude Code never gives up
/// on the hook before Delta's own fallback.
pub fn render_session_settings(
    port: u16,
    hook_secret: &str,
    permission_decision_deadline: Duration,
) -> String {
    let url = |path: &str| format!("http://127.0.0.1:{port}/hooks/{path}?hs={hook_secret}");
    let http_hook_with_timeout = |path: &str, timeout: u64| {
        json!({
            "hooks": [
                { "type": "http", "url": url(path), "timeout": timeout }
            ]
        })
    };
    let http_hook = |path: &str| http_hook_with_timeout(path, HOOK_TIMEOUT_SECS);
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
                    { "type": "http", "url": url("pre-tool-use"), "timeout": HOOK_TIMEOUT_SECS }
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
                    { "type": "http", "url": url("post-tool-use"), "timeout": HOOK_TIMEOUT_SECS }
                ]
            }],
            // PermissionRequest fires only when an interactive permission dialog
            // actually appears (not for auto/classifier-approved calls), so it is
            // the signal for "a human answer is genuinely pending". PreToolUse
            // fires for every call and only records the request. Its response
            // blocks on the browser's decision, so it gets the longer timeout
            // (see `permission_hook_timeout_secs`).
            "PermissionRequest": [http_hook_with_timeout(
                "permission-request",
                permission_hook_timeout_secs(permission_decision_deadline),
            )],
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
    use delta_usecase::PERMISSION_DECISION_DEADLINE;

    use super::*;

    fn render(port: u16, hook_secret: &str) -> String {
        render_session_settings(port, hook_secret, PERMISSION_DECISION_DEADLINE)
    }

    #[test]
    fn embeds_the_port_in_every_hook_url() {
        let rendered = render(9999, "sekret");
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();

        // The embedded terminal is dark, so the session's theme is forced dark.
        assert_eq!(parsed["theme"], "dark");

        assert_eq!(
            parsed["hooks"]["UserPromptSubmit"][0]["hooks"][0]["url"],
            "http://127.0.0.1:9999/hooks/user-prompt-submit?hs=sekret"
        );
        assert_eq!(
            parsed["hooks"]["Stop"][0]["hooks"][0]["url"],
            "http://127.0.0.1:9999/hooks/stop?hs=sekret"
        );
        assert_eq!(
            parsed["hooks"]["MessageDisplay"][0]["hooks"][0]["url"],
            "http://127.0.0.1:9999/hooks/message-display?hs=sekret"
        );
        assert_eq!(
            parsed["hooks"]["PreToolUse"][0]["hooks"][0]["url"],
            "http://127.0.0.1:9999/hooks/pre-tool-use?hs=sekret"
        );
        assert_eq!(parsed["hooks"]["PreToolUse"][0]["matcher"], "*");
        assert_eq!(
            parsed["hooks"]["PostToolUse"][0]["hooks"][0]["url"],
            "http://127.0.0.1:9999/hooks/post-tool-use?hs=sekret"
        );
        assert_eq!(parsed["hooks"]["PostToolUse"][0]["matcher"], "*");
        assert_eq!(
            parsed["hooks"]["PermissionRequest"][0]["hooks"][0]["url"],
            "http://127.0.0.1:9999/hooks/permission-request?hs=sekret"
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
        assert!(
            session_start_command.contains("http://127.0.0.1:9999/hooks/session-start?hs=sekret")
        );
        assert!(session_start_command.contains("--data-binary @-"));
        assert!(session_start_command.contains("-o /dev/null"));

        assert_eq!(
            parsed["hooks"]["SessionEnd"][0]["hooks"][0]["url"],
            "http://127.0.0.1:9999/hooks/session-end?hs=sekret"
        );
    }

    #[test]
    fn permission_request_hook_outlasts_the_decision_wait() {
        let rendered = render(9999, "sekret");
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        let timeout = |event: &str| {
            parsed["hooks"][event][0]["hooks"][0]["timeout"]
                .as_u64()
                .unwrap_or_else(|| panic!("{event} has a numeric timeout"))
        };

        assert!(
            timeout("PermissionRequest") > PERMISSION_DECISION_DEADLINE.as_secs(),
            "the PermissionRequest hook must outlast Delta's decision wait"
        );
        for event in [
            "UserPromptSubmit",
            "Stop",
            "MessageDisplay",
            "PreToolUse",
            "PostToolUse",
            "SessionEnd",
        ] {
            assert_eq!(timeout(event), 30, "{event} keeps the short timeout");
        }
    }

    #[test]
    fn permission_request_hook_timeout_follows_a_configured_wait() {
        // A wait stretched (or shrunk) through the environment keeps the hook
        // timeout above it, including a fractional one.
        for deadline in [
            Duration::from_millis(30),
            Duration::from_millis(90_500),
            Duration::from_secs(300),
        ] {
            let rendered = render_session_settings(9999, "sekret", deadline);
            let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();
            let timeout = parsed["hooks"]["PermissionRequest"][0]["hooks"][0]["timeout"]
                .as_u64()
                .unwrap();
            assert!(
                Duration::from_secs(timeout) > deadline,
                "timeout {timeout}s must outlast a {deadline:?} wait"
            );
        }
    }

    #[test]
    fn emits_a_status_line_command_targeting_the_same_port() {
        let rendered = render(9999, "sekret");
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();

        // `statusLine` is a top-level `command` entry (not a hook): Claude Code
        // pipes the status JSON to its stdin and renders its stdout as the
        // status line. It must target `/hooks/status-line` on the same port as
        // the hooks, forward stdin (`--data-binary @-`), and emit nothing to
        // stdout (`-o /dev/null`) so the embedded terminal's status line stays
        // empty.
        assert_eq!(parsed["statusLine"]["type"], "command");
        let command = parsed["statusLine"]["command"].as_str().unwrap();
        assert!(command.contains("http://127.0.0.1:9999/hooks/status-line?hs=sekret"));
        assert!(command.contains("--data-binary @-"));
        assert!(command.contains("-o /dev/null"));
    }
}
