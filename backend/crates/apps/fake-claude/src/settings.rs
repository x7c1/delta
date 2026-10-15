//! Extracting hook endpoint URLs from the `--settings` file.
//!
//! Delta renders a settings JSON whose `hooks` section points each event back
//! at the server, and passes it via `claude --settings <file>`. Most events
//! are native `http` hooks (`{"type":"http","url":...}`); `SessionStart` is a
//! `command` hook whose shell command `curl`s the same endpoint, reading the
//! URL from the `curl` configuration file it names with `-K`. The fake reads
//! the URL from either shape: it does not care *how* the real `claude` would
//! have delivered the event, only *where* the server listens for it.

use serde_json::Value;

/// The hook endpoints the fake fires, resolved from the settings file.
#[derive(Debug)]
pub struct HookEndpoints {
    pub session_start: String,
    pub user_prompt_submit: String,
    pub stop: String,
    pub message_display: String,
    pub pre_tool_use: String,
    pub post_tool_use: String,
    pub permission_request: String,
}

impl HookEndpoints {
    /// Resolve every endpoint the fake needs from the parsed settings JSON.
    pub fn from_settings(settings: &Value) -> Result<Self, String> {
        Ok(Self {
            session_start: hook_url(settings, "SessionStart")?,
            user_prompt_submit: hook_url(settings, "UserPromptSubmit")?,
            stop: hook_url(settings, "Stop")?,
            message_display: hook_url(settings, "MessageDisplay")?,
            pre_tool_use: hook_url(settings, "PreToolUse")?,
            post_tool_use: hook_url(settings, "PostToolUse")?,
            permission_request: hook_url(settings, "PermissionRequest")?,
        })
    }
}

/// The URL of the first hook configured for `event`.
///
/// An `http` hook carries it in `url`; a `command` hook's `curl` reads it from
/// the configuration file named after `-K`.
fn hook_url(settings: &Value, event: &str) -> Result<String, String> {
    let hook = settings["hooks"][event][0]["hooks"][0].clone();
    if let Some(url) = hook["url"].as_str() {
        return Ok(url.to_owned());
    }
    if let Some(command) = hook["command"].as_str() {
        let config = curl_config_path(command)
            .ok_or_else(|| format!("the {event} command names no `curl -K` file"))?;
        let text = std::fs::read_to_string(&config)
            .map_err(|e| format!("read the {event} curl configuration {config}: {e}"))?;
        return curl_config_url(&text)
            .ok_or_else(|| format!("the {event} curl configuration {config} holds no url"));
    }
    Err(format!("settings carry no resolvable {event} hook URL"))
}

/// The shell word after ` -K ` in `command`, with its quoting removed: a
/// single-quoted run is taken literally and a backslash escapes the next
/// character, which covers the `'…'\''…'` form Delta quotes paths in.
fn curl_config_path(command: &str) -> Option<String> {
    let (_, rest) = command.split_once(" -K ")?;
    let mut word = String::new();
    let mut chars = rest.trim_start().chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => word.extend(chars.by_ref().take_while(|&c| c != '\'')),
            '\\' => word.extend(chars.next()),
            c if c.is_whitespace() => break,
            c => word.push(c),
        }
    }
    (!word.is_empty()).then_some(word)
}

/// The `url = "…"` value of a `curl` configuration file, unescaped.
fn curl_config_url(text: &str) -> Option<String> {
    let value = text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "url").then(|| value.trim())
    })?;
    let quoted = value.strip_prefix('"')?.strip_suffix('"')?;
    let mut url = String::new();
    let mut chars = quoted.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => url.extend(chars.next()),
            c => url.push(c),
        }
    }
    Some(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings(session_start_config: &str) -> Value {
        json!({
            "hooks": {
                "UserPromptSubmit": [
                    { "hooks": [ { "type": "http", "url": "http://127.0.0.1:7878/hooks/user-prompt-submit" } ] }
                ],
                "Stop": [
                    { "hooks": [ { "type": "http", "url": "http://127.0.0.1:7878/hooks/stop" } ] }
                ],
                "MessageDisplay": [
                    { "hooks": [ { "type": "http", "url": "http://127.0.0.1:7878/hooks/message-display" } ] }
                ],
                "PreToolUse": [
                    { "matcher": "*", "hooks": [ { "type": "http", "url": "http://127.0.0.1:7878/hooks/pre-tool-use" } ] }
                ],
                "PostToolUse": [
                    { "matcher": "*", "hooks": [ { "type": "http", "url": "http://127.0.0.1:7878/hooks/post-tool-use" } ] }
                ],
                "PermissionRequest": [
                    { "hooks": [ { "type": "http", "url": "http://127.0.0.1:7878/hooks/permission-request" } ] }
                ],
                "SessionStart": [
                    { "hooks": [ { "type": "command", "command": format!(
                        "curl -sS -m 5 -o /dev/null -X POST -K {session_start_config} \
                         -H 'content-type: application/json' --data-binary @-"
                    ) } ] }
                ]
            }
        })
    }

    /// A `curl` configuration for the SessionStart endpoint in a directory
    /// whose name needs quoting, and the settings whose command names it the
    /// way Delta quotes it.
    fn settings_with_config() -> (tempfile::TempDir, Value) {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("Delta's data");
        std::fs::create_dir(&config_dir).unwrap();
        let config = config_dir.join("7878-session-start.curl");
        std::fs::write(
            &config,
            "url = \"http://127.0.0.1:7878/hooks/session-start?hs=abc\"\n",
        )
        .unwrap();
        let quoted = format!("'{}'", config.to_str().unwrap().replace('\'', r"'\''"));
        let settings = settings(&quoted);
        (dir, settings)
    }

    #[test]
    fn resolves_http_hooks_from_their_url() {
        let (_dir, settings) = settings_with_config();
        let endpoints = HookEndpoints::from_settings(&settings).unwrap();
        assert_eq!(
            endpoints.user_prompt_submit,
            "http://127.0.0.1:7878/hooks/user-prompt-submit"
        );
        assert_eq!(endpoints.stop, "http://127.0.0.1:7878/hooks/stop");
    }

    #[test]
    fn resolves_the_command_hook_from_its_curl_config() {
        let (_dir, settings) = settings_with_config();
        let endpoints = HookEndpoints::from_settings(&settings).unwrap();
        assert_eq!(
            endpoints.session_start,
            "http://127.0.0.1:7878/hooks/session-start?hs=abc"
        );
    }

    #[test]
    fn a_missing_event_is_a_readable_error() {
        let err = HookEndpoints::from_settings(&json!({"hooks": {}})).unwrap_err();
        assert!(err.contains("SessionStart"));
    }
}
