//! Extracting hook endpoint URLs from the `--settings` file.
//!
//! Delta renders a settings JSON whose `hooks` section points each event back
//! at the server, and passes it via `claude --settings <file>`. Most events
//! are native `http` hooks (`{"type":"http","url":...}`); `SessionStart` is a
//! `command` hook whose shell command `curl`s the same endpoint. The fake
//! reads the URL from either shape: it does not care *how* the real `claude`
//! would have delivered the event, only *where* the server listens for it.

use serde_json::Value;

/// The hook endpoints the fake fires, resolved from the settings file.
#[derive(Debug)]
pub struct HookEndpoints {
    pub session_start: String,
    pub user_prompt_submit: String,
    pub stop: String,
    pub pre_tool_use: String,
    pub permission_request: String,
}

impl HookEndpoints {
    /// Resolve every endpoint the fake needs from the parsed settings JSON.
    pub fn from_settings(settings: &Value) -> Result<Self, String> {
        Ok(Self {
            session_start: hook_url(settings, "SessionStart")?,
            user_prompt_submit: hook_url(settings, "UserPromptSubmit")?,
            stop: hook_url(settings, "Stop")?,
            pre_tool_use: hook_url(settings, "PreToolUse")?,
            permission_request: hook_url(settings, "PermissionRequest")?,
        })
    }
}

/// The URL of the first hook configured for `event`.
///
/// An `http` hook carries it in `url`; a `command` hook embeds it in the
/// `curl` command line, where it is the first `http://…` token.
fn hook_url(settings: &Value, event: &str) -> Result<String, String> {
    let hook = settings["hooks"][event][0]["hooks"][0].clone();
    if let Some(url) = hook["url"].as_str() {
        return Ok(url.to_owned());
    }
    if let Some(command) = hook["command"].as_str() {
        if let Some(url) = first_http_token(command) {
            return Ok(url);
        }
    }
    Err(format!("settings carry no resolvable {event} hook URL"))
}

/// The first whitespace-delimited `http://…` token of a command line.
fn first_http_token(command: &str) -> Option<String> {
    command
        .split_whitespace()
        .find(|token| token.starts_with("http://"))
        .map(|token| token.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings() -> Value {
        json!({
            "hooks": {
                "UserPromptSubmit": [
                    { "hooks": [ { "type": "http", "url": "http://127.0.0.1:7878/hooks/user-prompt-submit" } ] }
                ],
                "Stop": [
                    { "hooks": [ { "type": "http", "url": "http://127.0.0.1:7878/hooks/stop" } ] }
                ],
                "PreToolUse": [
                    { "matcher": "*", "hooks": [ { "type": "http", "url": "http://127.0.0.1:7878/hooks/pre-tool-use" } ] }
                ],
                "PermissionRequest": [
                    { "hooks": [ { "type": "http", "url": "http://127.0.0.1:7878/hooks/permission-request" } ] }
                ],
                "SessionStart": [
                    { "hooks": [ { "type": "command", "command": "curl -sS -m 5 -o /dev/null -X POST http://127.0.0.1:7878/hooks/session-start -H 'content-type: application/json' --data-binary @-" } ] }
                ]
            }
        })
    }

    #[test]
    fn resolves_http_hooks_from_their_url() {
        let endpoints = HookEndpoints::from_settings(&settings()).unwrap();
        assert_eq!(
            endpoints.user_prompt_submit,
            "http://127.0.0.1:7878/hooks/user-prompt-submit"
        );
        assert_eq!(endpoints.stop, "http://127.0.0.1:7878/hooks/stop");
    }

    #[test]
    fn resolves_the_command_hook_from_its_curl_command() {
        let endpoints = HookEndpoints::from_settings(&settings()).unwrap();
        assert_eq!(
            endpoints.session_start,
            "http://127.0.0.1:7878/hooks/session-start"
        );
    }

    #[test]
    fn a_missing_event_is_a_readable_error() {
        let err = HookEndpoints::from_settings(&json!({"hooks": {}})).unwrap_err();
        assert!(err.contains("SessionStart"));
    }
}
