//! Claude Code text-format detection, in one place.
//!
//! These are the textual conventions Claude Code uses on the wire Delta
//! observes (the JSONL transcript and the hook payloads), gathered here so
//! attribution and the hook handlers share one definition. The structural
//! flags (e.g. `is_queued_command`) are already detected by the transcript
//! parser in the gateway; these cover the conventions that are plain strings.

mod forked_skill_launch;
pub use forked_skill_launch::{forked_skill_launch, has_forked_skill_launch, ForkedSkillLaunch};

/// Prefix Claude Code writes to the transcript when the user interrupts the
/// in-flight turn. It appears as a `role: user` line whose only text block is
/// either `[Request interrupted by user]` (plain mid-response interrupt) or
/// `[Request interrupted by user for tool use]` (interrupt during a tool use).
/// Matching on the shared prefix covers both variants (and any future suffix)
/// without enumerating each exact string.
const INTERRUPT_MARKER_PREFIX: &str = "[Request interrupted by user";

/// Prompt prefix Claude Code uses when it injects a background-task
/// completion notification. Such a submission is a harness injection, not a
/// human typing into the pane, so it must not be reported as external input.
const TASK_NOTIFICATION_PREFIX: &str = "<task-notification>";

/// Prefix of the caveat line Claude Code writes as the first member of a
/// slash/local-command group (e.g. when the user runs `/review-pr`). Claude
/// records the group as several `type: "user"` lines that all share one
/// `promptId`: this caveat (flagged `isMeta`), then the bare command-name line
/// (e.g. `/review-pr`), then the command's `<local-command-stdout>` /
/// `<local-command-stderr>` output. Only the caveat carries `isMeta`, so the
/// other members would otherwise render as human user turns. Recognizing the
/// caveat by this prefix is what lets attribution group the whole sequence by
/// shared `promptId`.
const LOCAL_COMMAND_CAVEAT_PREFIX: &str = "<local-command-caveat>";

/// Prefixes of a local command's captured output, written by Claude Code as
/// `type: "user"` lines with NO `isMeta` flag. They are command machinery, not
/// human turns, so attribution folds them to [`crate::Role::Meta`]. Detected by
/// content because — unlike the bare command-name line — the prefix is a stable
/// structural marker, so this catches the output even outside a recognized
/// `promptId` group (e.g. a partial sync window that missed the caveat line).
const LOCAL_COMMAND_OUTPUT_PREFIXES: [&str; 2] =
    ["<local-command-stdout>", "<local-command-stderr>"];

/// Whether a (trimmed) user-line text is the interrupt marker Claude Code
/// writes when the user aborts the in-flight turn.
pub fn is_interrupt_marker(trimmed_text: &str) -> bool {
    trimmed_text.starts_with(INTERRUPT_MARKER_PREFIX)
}

/// Whether a hook-submitted prompt is a harness-injected task notification
/// rather than something typed into the pane.
pub fn is_task_notification(prompt: &str) -> bool {
    prompt.trim_start().starts_with(TASK_NOTIFICATION_PREFIX)
}

/// Whether a (trimmed) user-line text is the caveat Claude Code writes as the
/// first member of a slash/local-command group. The caveat itself is flagged
/// `isMeta` by Claude; recognizing it here lets attribution treat every other
/// line sharing its `promptId` (the bare command-name line and the captured
/// `<local-command-stdout>`/`<local-command-stderr>`) as command machinery.
pub fn is_local_command_caveat(trimmed_text: &str) -> bool {
    trimmed_text.starts_with(LOCAL_COMMAND_CAVEAT_PREFIX)
}

/// Whether a (trimmed) user-line text is a local command's captured output
/// (`<local-command-stdout>` / `<local-command-stderr>`). Claude records these
/// as `type: "user"` lines without `isMeta`, so without this they would render
/// as human user turns.
pub fn is_local_command_output(trimmed_text: &str) -> bool {
    LOCAL_COMMAND_OUTPUT_PREFIXES
        .iter()
        .any(|prefix| trimmed_text.starts_with(prefix))
}

/// Normalize a slash-command token to its bare command name: strip a single
/// leading `/`, then — if a namespace prefix is present — drop everything up to
/// and including the LAST `:` (so `example:` is discarded). The remainder
/// is returned unchanged when there is no `:`.
///
/// Examples: `/example:review-pr` -> `review-pr`; `/review-pr` ->
/// `review-pr`; `review-pr` -> `review-pr`.
fn bare_command_name(token: &str) -> &str {
    let without_slash = token.strip_prefix('/').unwrap_or(token);
    match without_slash.rfind(':') {
        Some(idx) => &without_slash[idx + 1..],
        None => without_slash,
    }
}

/// Whether a dispatched send is a slash command — i.e. its first
/// whitespace-delimited token starts with `/`.
///
/// This is the guard the fold's two command branches consume a send by. A
/// slash-command send produces no `UserPromptSubmit` echo and no `Stop`:
/// Claude Code handles it client-side and records either a local-command
/// group or an unknown-command notice, so one of those lines is the ONLY
/// signal Delta gets that the send was consumed, whatever command NAME it
/// ended up recording. A PLAIN-prompt send does echo, so a command line
/// showing up while one is outstanding means something else was submitted
/// into the pane, and consuming the send would silently drop the user's
/// message.
pub fn is_slash_command_send(text: &str) -> bool {
    text.split_whitespace()
        .next()
        .is_some_and(|token| token.starts_with('/'))
}

/// Whether a local-command command-name line spells the SAME command as the
/// dispatched send, tolerating the namespace prefix Claude Code adds to the
/// transcript.
///
/// This reports; it does not gate. Consumption of the send is positional and
/// guarded by [`is_slash_command_send`]; this verdict rides along as
/// `Effect::SendMatched::attributed`, so a name Claude rewrote into a shape
/// Delta has not catalogued shows up in the log the first time it happens.
///
/// The one rewrite already catalogued is the namespace prefix: when a user
/// types a short command such as `/review-pr`, Delta dispatches the send with
/// exactly that text, but Claude Code may expand it to its fully-qualified
/// form (e.g. `/example:review-pr`) in the transcript's bare command-name
/// line. Comparing raw text would call that an unattributed line, so the
/// comparison is on BARE command names — which also makes it symmetric,
/// matching regardless of which side carries the prefix.
///
/// Only the FIRST whitespace-delimited token of each side is considered;
/// trailing args are ignored. This is safe under the single-outstanding-send
/// rule (there is at most one send to correlate against) and mirrors the
/// unknown-command notice comparison, which likewise looks at the first
/// token. Returns `false` when either side has no first token.
pub fn local_command_name_line_matches_send(send_text: &str, name_line: &str) -> bool {
    match (
        send_text.split_whitespace().next(),
        name_line.split_whitespace().next(),
    ) {
        (Some(send_token), Some(line_token)) => {
            bare_command_name(send_token) == bare_command_name(line_token)
        }
        _ => false,
    }
}

/// Opening of the placeholder Claude Code substitutes for an image attachment,
/// e.g. `[Image #2]`. Claude Code's composer spots an image file path in the
/// text typed into it, reads the file, and replaces the path with this
/// placeholder — hoisted to the FRONT of the submitted prompt — before the
/// prompt reaches the `UserPromptSubmit` hook and the transcript. The number
/// is a session-wide counter, not a per-message index, so it says nothing
/// about how many attachments this message carries.
const IMAGE_PLACEHOLDER_OPEN: &str = "[Image #";

/// File extensions whose paths Claude Code turns into an `[Image #N]`
/// attachment. Deliberately narrow — the same media types the placeholder is
/// emitted for — so that stripping a "path line" out of a send stays a
/// conservative operation rather than a guess about arbitrary text.
const ATTACHMENT_IMAGE_EXTENSIONS: [&str; 5] = [".png", ".jpg", ".jpeg", ".gif", ".webp"];

/// Whether a `UserPromptSubmit` prompt (or a transcript user line) is the echo
/// of `send_text` — the message Delta typed into the pane.
///
/// Plain text echoes back byte-for-byte, so the primary rule is what it has
/// always been: exact equality after trimming.
///
/// An **image-attachment** send does not: the composed text carries the
/// attachment's path on its own line, and Claude Code's composer swallows that
/// path, reads the file, and submits `[Image #N]<body>` instead — so exact
/// equality can never hold and the send would be treated as unechoed forever.
/// The second rule recognizes that rewrite: strip the leading `[Image #N]`
/// placeholders from the prompt, strip the attachment path lines from the
/// send, and compare what is left.
///
/// It is deliberately conservative — a mismatch is always the safe answer here,
/// because a false *positive* would attribute someone else's typing to the
/// user's composed message:
///
/// - at least one placeholder must be present, and the number of placeholders
///   must equal the number of path lines removed (a partially-recognized
///   attachment set does not match);
/// - a path line must be absolute, carry an image extension, and contain no
///   unescaped whitespace;
/// - the remaining bodies must be equal line-for-line (each line trimmed, blank
///   lines dropped) — Claude Code drops the newline that separated the body
///   from the path, so the comparison cannot be raw equality, but it is not
///   loosened any further than that.
///
/// A send with no attachment path line therefore takes exactly the old
/// exact-match path. Slash commands are untouched as well: a local-command
/// name line is resolved in its own branch at the call site, guarded by
/// [`is_slash_command_send`] and reported on by
/// [`local_command_name_line_matches_send`].
pub fn prompt_echoes_send(send_text: &str, prompt: &str) -> bool {
    send_text.trim() == prompt.trim() || attachment_echo_matches_send(send_text, prompt)
}

/// The attachment-aware half of [`prompt_echoes_send`]. Separate so the
/// exact-match fast path stays obvious at the call site above.
fn attachment_echo_matches_send(send_text: &str, prompt: &str) -> bool {
    let (placeholders, prompt_body) = strip_leading_image_placeholders(prompt);
    if placeholders == 0 {
        return false;
    }
    let mut attachments = 0;
    let mut send_body = Vec::new();
    for line in body_lines(send_text) {
        if is_attachment_path_line(line) {
            attachments += 1;
        } else {
            send_body.push(line);
        }
    }
    attachments == placeholders && send_body == body_lines(prompt_body)
}

/// Split a prompt into the count of `[Image #N]` placeholders it opens with and
/// the body that follows them. Only LEADING placeholders count: that is where
/// Claude Code puts them, and refusing to scan the whole text keeps a body that
/// merely mentions the placeholder from being mistaken for an attachment.
fn strip_leading_image_placeholders(prompt: &str) -> (usize, &str) {
    let mut rest = prompt.trim_start();
    let mut count = 0;
    while let Some(after) = strip_image_placeholder(rest) {
        count += 1;
        rest = after.trim_start();
    }
    (count, rest)
}

/// Strip one `[Image #<digits>]` placeholder from the front of `text`,
/// returning the remainder. `None` when the text does not open with one.
fn strip_image_placeholder(text: &str) -> Option<&str> {
    let rest = text.strip_prefix(IMAGE_PLACEHOLDER_OPEN)?;
    let end = rest.find(|c: char| !c.is_ascii_digit())?;
    if end == 0 {
        return None;
    }
    rest[end..].strip_prefix(']')
}

/// The comparable body of a text: its lines trimmed, with blank lines dropped.
///
/// Claude Code's rewrite does not preserve the whitespace around the swallowed
/// path (the newline that separated it from the body disappears), so the two
/// sides are compared as line sequences rather than as raw strings.
fn body_lines(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

/// Whether a (trimmed) line of a send is the path of an image attachment
/// Claude Code would swallow: an absolute path, with every space
/// backslash-escaped (the shell-escaped form a path dragged out of a file
/// manager takes), naming one of [`ATTACHMENT_IMAGE_EXTENSIONS`].
fn is_attachment_path_line(line: &str) -> bool {
    if !line.starts_with('/') {
        return false;
    }
    // Reject any whitespace that is not a backslash-escaped space: a line with
    // bare spaces is prose that happens to start with a slash, not a path.
    let mut escaped = false;
    for ch in line.chars() {
        if ch.is_whitespace() && !(ch == ' ' && escaped) {
            return false;
        }
        escaped = ch == '\\' && !escaped;
    }
    let lower = line.to_ascii_lowercase();
    ATTACHMENT_IMAGE_EXTENSIONS
        .iter()
        .any(|extension| lower.ends_with(extension))
}

/// Prefix of the notice Claude Code writes when the user types a slash command
/// it does not recognize (e.g. `/review-pr` when no such command exists). Claude
/// records it as a single `type: "system"` / `subtype: "informational"` line
/// whose top-level content is `Unknown command: <command>`. Unlike a KNOWN local
/// command, an unknown command fires NEITHER a `UserPromptSubmit` echo NOR a
/// `Stop` hook and writes no `user`/`assistant` line — only this warning. Delta
/// dispatched the command as a send and moved the turn machine to `AwaitingEcho`,
/// so without recognizing this notice the send wedges the single-outstanding
/// queue forever. Attribution keys on this prefix to end the degenerate turn
/// client-side, mirroring the known-local-command handling.
const UNKNOWN_COMMAND_NOTICE_PREFIX: &str = "Unknown command:";

/// Whether a (trimmed) system-line content is the unknown-command notice Claude
/// Code writes for an unrecognized slash command.
pub fn is_unknown_command_notice(trimmed_text: &str) -> bool {
    trimmed_text.starts_with(UNKNOWN_COMMAND_NOTICE_PREFIX)
}

/// The command an unknown-command notice names, e.g. `/review-pr` from
/// `Unknown command: /review-pr`. Returns `None` when the text is not an
/// unknown-command notice or names no command after the prefix.
///
/// The notice names just the command (no args), while the send Delta dispatched
/// may carry args (`/review-pr 123`), so the caller compares this token against
/// the outstanding send's FIRST whitespace-delimited token rather than against
/// the whole send text. That comparison only feeds
/// `Effect::SendMatched::attributed`; consumption of the send is positional and
/// guarded by [`is_slash_command_send`], so a `None` here (a notice naming no
/// command at all) still consumes an outstanding slash-command send, reporting
/// it unattributed.
pub fn unknown_command_from_notice(trimmed_text: &str) -> Option<&str> {
    let rest = trimmed_text
        .strip_prefix(UNKNOWN_COMMAND_NOTICE_PREFIX)?
        .trim();
    (!rest.is_empty()).then_some(rest)
}

/// The `<tool-use-id>` element a `<task-notification>` body carries: the id of
/// the `Agent`/`Task`/`Bash` tool call whose background completion this
/// notification reports. It equals the launching tool_use `id` (the
/// `toolu_...` value), so it is the correlation key from a completion back to
/// the thread that launched the task.
///
/// Returns `None` when the text is not a task notification or carries no
/// `<tool-use-id>` element. The extraction is a minimal element scan rather
/// than a full XML parse: the body is a flat, harness-generated block and the
/// element value never contains markup.
pub fn task_notification_tool_use_id(prompt: &str) -> Option<&str> {
    task_notification_element(prompt, "tool-use-id")
}

/// The `<task-id>` element a `<task-notification>` body carries: the
/// background-task identifier Claude Code mints for the subagent, separate
/// from the launching `<tool-use-id>`. Recent Claude Code versions sometimes
/// drop `<tool-use-id>` from the user-message notification body while keeping
/// `<task-id>`, so it serves as a fallback correlation key when matching a
/// completion back to its launching `RunningSubagent` entry.
///
/// Returns `None` when the text is not a task notification or carries no
/// `<task-id>` element. Mirrors [`task_notification_tool_use_id`]: a minimal
/// element scan over the flat, harness-generated body.
pub fn task_notification_task_id(prompt: &str) -> Option<&str> {
    task_notification_element(prompt, "task-id")
}

/// Inner-text extractor shared by [`task_notification_tool_use_id`] and
/// [`task_notification_task_id`]. The body is gated on the task-notification
/// prefix, then scanned for `<name>...</name>` — a minimal lookup that suits a
/// flat, harness-generated block without pulling in a full XML parse.
fn task_notification_element<'a>(prompt: &'a str, name: &str) -> Option<&'a str> {
    if !is_task_notification(prompt) {
        return None;
    }
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = prompt.find(&open)? + open.len();
    let rest = &prompt[start..];
    let end = rest.find(&close)?;
    Some(rest[..end].trim())
}

/// Fallback used during fold to capture the `agentId: <id>` substring from
/// the launch tool_result text, because the JSONL sibling
/// `toolUseResult.agentId` is not preserved in `ContentBlock::ToolResult`.
///
/// Claude Code writes the background-task identifier into the human-readable
/// `tool_result` text (`Async agent launched successfully.\nagentId: <id> ...`)
/// alongside a sibling `toolUseResult.agentId` field on the same JSONL line.
/// The sibling carries the same value, but the structural parser only keeps
/// the `content` blocks, so the in-memory `tool_result` Delta sees has only
/// the text. Recovering the id from that text gives the fold path the same
/// `task_id` upgrade the live `PostToolUse(Agent)` hook records — needed when
/// a `<task-notification>` body ships only `<task-id>`.
///
/// Accepts the three shapes `content` realistically takes: an array of
/// `{ "type": "text", "text": "..." }` blocks (the typical Claude shape), a
/// single such object, or a plain JSON string. Anything else degrades to
/// `None`. The id token is `[A-Za-z0-9_-]+`; the first `agentId: ` occurrence
/// wins.
pub fn agent_id_from_tool_result_content(content: &serde_json::Value) -> Option<&str> {
    if let Some(s) = content.as_str() {
        return extract_agent_id_from_text(s);
    }
    if let Some(arr) = content.as_array() {
        for block in arr {
            if let Some(text) = block.get("text").and_then(serde_json::Value::as_str) {
                if let Some(id) = extract_agent_id_from_text(text) {
                    return Some(id);
                }
            }
        }
        return None;
    }
    content
        .get("text")
        .and_then(serde_json::Value::as_str)
        .and_then(extract_agent_id_from_text)
}

/// Scan a plain text string for the first `agentId: <id>` token and return
/// the id (`[A-Za-z0-9_-]+`). Returns `None` if the marker is absent or no id
/// characters follow it.
fn extract_agent_id_from_text(text: &str) -> Option<&str> {
    const MARKER: &str = "agentId: ";
    let start = text.find(MARKER)? + MARKER.len();
    let rest = &text[start..];
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(rest.len());
    if end == 0 {
        None
    } else {
        Some(&rest[..end])
    }
}

/// Whether a tool_use launches the tool in the background — i.e. returns
/// immediately while the actual work continues, with a later
/// `<task-notification>` user line reporting its completion. The launching
/// tool_use `id` is recorded as the correlation key for that notification.
///
/// Modern Claude Code makes `Agent`/`Task` calls async by default and dropped
/// the `run_in_background` parameter from their schema, so for those tools the
/// absence of the key means background, not foreground. An explicit
/// `run_in_background: false` is still respected for forward compatibility, and
/// `true` is honoured for any tool (so a Bash invocation still has to opt in
/// explicitly — its default is foreground). Any other tool with no explicit
/// flag stays foreground.
pub fn launches_in_background(tool_name: &str, tool_use_input: &serde_json::Value) -> bool {
    let explicit = tool_use_input
        .get("run_in_background")
        .and_then(serde_json::Value::as_bool);
    match (tool_name, explicit) {
        (_, Some(b)) => b,
        ("Agent" | "Task", None) => true,
        _ => false,
    }
}

/// The tool names that spawn a subagent.
///
/// The current Claude Code build names this tool `Agent`; older builds named it
/// `Task`. Matched defensively against both so the hook contract / transcript
/// content can drift without breaking attribution.
pub const SUBAGENT_TOOL_NAMES: [&str; 2] = ["Agent", "Task"];

/// Whether `tool_name` names a subagent-spawning tool (see
/// [`SUBAGENT_TOOL_NAMES`]).
pub fn is_subagent_tool(tool_name: &str) -> bool {
    SUBAGENT_TOOL_NAMES.contains(&tool_name)
}

/// Read an optional non-empty string field out of a JSON payload — an
/// `Agent`/`Task` tool input, or a `<forked-skill-launch>` body.
///
/// Returns `None` when the payload is not an object, the key is missing, the
/// value is not a string, or the string is empty — so a malformed or partial
/// payload degrades to "no label" rather than failing.
pub fn json_string_field<'a>(payload: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    payload.get(key)?.as_str().filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_interrupt_marker_variants_match_by_prefix() {
        assert!(is_interrupt_marker("[Request interrupted by user]"));
        assert!(is_interrupt_marker(
            "[Request interrupted by user for tool use]"
        ));
        assert!(!is_interrupt_marker("a normal prompt"));
        assert!(!is_interrupt_marker(""));
    }

    #[test]
    fn task_notification_is_detected_with_leading_whitespace() {
        assert!(is_task_notification(
            "<task-notification>done</task-notification>"
        ));
        assert!(is_task_notification("  <task-notification>done"));
        assert!(!is_task_notification("a normal prompt"));
    }

    #[test]
    fn tool_use_id_is_extracted_from_a_task_notification_body() {
        let body = "<task-notification>\n\
                    <task-id>a31425032172620ed</task-id>\n\
                    <tool-use-id>toolu_01PqcdgEeMZekxvwSqjBviuA</tool-use-id>\n\
                    <output-file>/tmp/x.output</output-file>\n\
                    <status>completed</status>\n\
                    </task-notification>";
        assert_eq!(
            task_notification_tool_use_id(body),
            Some("toolu_01PqcdgEeMZekxvwSqjBviuA")
        );
    }

    #[test]
    fn tool_use_id_extraction_ignores_non_notifications_and_missing_element() {
        // Not a task notification at all.
        assert_eq!(
            task_notification_tool_use_id("<tool-use-id>toolu_x</tool-use-id>"),
            None
        );
        // A notification with no `<tool-use-id>` element (e.g. malformed).
        assert_eq!(
            task_notification_tool_use_id(
                "<task-notification><status>completed</status></task-notification>"
            ),
            None
        );
    }

    #[test]
    fn task_id_is_extracted_from_a_task_notification_body() {
        let body = "<task-notification>\n\
                    <task-id>a31425032172620ed</task-id>\n\
                    <tool-use-id>toolu_01PqcdgEeMZekxvwSqjBviuA</tool-use-id>\n\
                    <output-file>/tmp/x.output</output-file>\n\
                    <status>completed</status>\n\
                    </task-notification>";
        assert_eq!(task_notification_task_id(body), Some("a31425032172620ed"));
    }

    #[test]
    fn task_id_is_extracted_when_tool_use_id_is_missing() {
        // The motivating case: recent Claude Code versions sometimes drop
        // `<tool-use-id>` from the body while keeping `<task-id>`. The fallback
        // key must still come back here.
        let body = "<task-notification>\n\
                    <task-id>a31425032172620ed</task-id>\n\
                    <status>completed</status>\n\
                    </task-notification>";
        assert_eq!(
            task_notification_tool_use_id(body),
            None,
            "the tool-use-id element really is absent"
        );
        assert_eq!(task_notification_task_id(body), Some("a31425032172620ed"));
    }

    #[test]
    fn task_id_extraction_ignores_non_notifications_and_missing_element() {
        assert_eq!(
            task_notification_task_id("<task-id>a31425032172620ed</task-id>"),
            None
        );
        assert_eq!(
            task_notification_task_id(
                "<task-notification><status>completed</status></task-notification>"
            ),
            None
        );
    }

    #[test]
    fn agent_id_is_extracted_from_an_array_of_text_blocks() {
        let content = serde_json::json!([
            {
                "type": "text",
                "text": "Async agent launched successfully.\n\
                         agentId: a6a7d31c908cdfa24 (internal ID - do not mention to user.)\n\
                         The agent is working in the background.",
            }
        ]);
        assert_eq!(
            agent_id_from_tool_result_content(&content),
            Some("a6a7d31c908cdfa24")
        );
    }

    #[test]
    fn agent_id_extraction_returns_none_when_marker_is_absent() {
        let content = serde_json::json!([
            { "type": "text", "text": "no marker here, just regular tool output." }
        ]);
        assert_eq!(agent_id_from_tool_result_content(&content), None);
    }

    #[test]
    fn agent_id_extraction_degrades_for_non_string_and_malformed_content() {
        // Not an array, object, or string — a bare number is unknown shape.
        assert_eq!(
            agent_id_from_tool_result_content(&serde_json::json!(42)),
            None
        );
        // Object missing the `text` field.
        assert_eq!(
            agent_id_from_tool_result_content(&serde_json::json!({ "type": "text" })),
            None
        );
    }

    #[test]
    fn local_command_caveat_is_detected_by_prefix() {
        assert!(is_local_command_caveat(
            "<local-command-caveat>Caveat: The messages below were generated by the user \
             while running local commands. DO NOT respond to these messages...</local-command-caveat>"
        ));
        assert!(!is_local_command_caveat("/review-pr"));
        assert!(!is_local_command_caveat("a normal prompt"));
        assert!(!is_local_command_caveat(""));
    }

    #[test]
    fn local_command_output_matches_stdout_and_stderr() {
        assert!(is_local_command_output(
            "<local-command-stdout>\nPENDING review created.\n</local-command-stdout>"
        ));
        assert!(is_local_command_output(
            "<local-command-stderr>boom</local-command-stderr>"
        ));
        // The bare command-name line is not output: it carries no marker and is
        // grouped by shared promptId, not by content.
        assert!(!is_local_command_output("/review-pr"));
        assert!(!is_local_command_output("a normal prompt"));
        assert!(!is_local_command_output(""));
    }

    #[test]
    fn bare_command_name_strips_slash_and_namespace_prefix() {
        assert_eq!(bare_command_name("/example:review-pr"), "review-pr");
        assert_eq!(bare_command_name("/review-pr"), "review-pr");
        assert_eq!(bare_command_name("review-pr"), "review-pr");
    }

    #[test]
    fn is_slash_command_send_looks_at_the_first_token_only() {
        // The plain shapes: a bare command, a command with args, and a
        // namespaced command are all slash commands.
        assert!(is_slash_command_send("/review-pr"));
        assert!(is_slash_command_send("/review-pr 123"));
        assert!(is_slash_command_send("/example:review-pr"));
        // A typo is still a slash command — the whole point of the guard is
        // that the NAME does not matter, only the kind.
        assert!(is_slash_command_send("/revew-pr"));
        // Leading whitespace is not significant.
        assert!(is_slash_command_send("  \n/review-pr\n"));
        // A plain prompt is not, even when it mentions a slash later on.
        assert!(!is_slash_command_send("hello world"));
        assert!(!is_slash_command_send("run /review-pr for me"));
        // A path-looking prompt still counts: it starts with `/`, and Claude
        // Code would reject it as an unknown command rather than echo it.
        assert!(is_slash_command_send("/tmp/notes.txt"));
        // No first token at all.
        assert!(!is_slash_command_send(""));
        assert!(!is_slash_command_send("   "));
    }

    #[test]
    fn local_command_name_line_matches_send_tolerates_namespace() {
        // The motivating case: Claude expands the short form the user typed
        // (and thus the dispatched send) to its fully-qualified namespaced form
        // in the transcript command-name line.
        assert!(local_command_name_line_matches_send(
            "/review-pr",
            "/example:review-pr"
        ));
        // Identical short-vs-short and full-vs-full both match.
        assert!(local_command_name_line_matches_send(
            "/review-pr",
            "/review-pr"
        ));
        assert!(local_command_name_line_matches_send(
            "/example:review-pr",
            "/example:review-pr"
        ));
        // Symmetry: a fully-qualified send against a short line matches too.
        assert!(local_command_name_line_matches_send(
            "/example:review-pr",
            "/review-pr"
        ));
        // Different commands do NOT match even under a shared namespace.
        assert!(!local_command_name_line_matches_send(
            "/review-pr",
            "/example:other"
        ));
        // Args after the command are ignored (single-outstanding-send rule).
        assert!(local_command_name_line_matches_send(
            "/review-pr 123",
            "/example:review-pr"
        ));
        // Empty / no-first-token inputs return false.
        assert!(!local_command_name_line_matches_send("", "/review-pr"));
        assert!(!local_command_name_line_matches_send("/review-pr", ""));
        assert!(!local_command_name_line_matches_send("   ", "/review-pr"));
    }

    #[test]
    fn plain_text_echo_matching_is_exact_equality_after_trimming() {
        assert!(prompt_echoes_send("hello world", "hello world"));
        assert!(prompt_echoes_send("  hello world\n", "hello world"));
        assert!(!prompt_echoes_send("hello world", "hello  world"));
        assert!(!prompt_echoes_send("hello world", "typed straight in"));
        // A body that merely mentions the placeholder is still plain text: with
        // no attachment path line in the send there is nothing to strip, so the
        // exact-match semantics decide (and here they disagree).
        assert!(!prompt_echoes_send(
            "what does [Image #2] mean",
            "[Image #2] mean"
        ));
    }

    #[test]
    fn image_attachment_echo_matches_the_send_that_carried_its_path() {
        // The observed incident shape: Delta types the body plus the attachment
        // path (shell-escaped spaces) on its own line; Claude Code swallows the
        // path, reads the file, and submits the body behind an `[Image #N]`
        // placeholder — with the separating newline gone.
        let send = "can you read this picture\n/home/dev/pictures/Screenshot\\ 2026-08-09\\ at\\ 11.51.52.png";
        assert!(prompt_echoes_send(
            send,
            "[Image #2]can you read this picture"
        ));
        // The placeholder may be followed by whitespace instead of running
        // straight into the body.
        assert!(prompt_echoes_send(
            send,
            "[Image #2]\n can you read this picture"
        ));
        // A different body is still a mismatch: only the attachment rewrite is
        // tolerated, never the message itself changing.
        assert!(!prompt_echoes_send(
            send,
            "[Image #2]something else entirely"
        ));
    }

    #[test]
    fn multiple_attachments_match_when_every_path_has_a_placeholder() {
        let send = "compare these\n/home/dev/pictures/before.png\n/home/dev/pictures/after.jpeg";
        assert!(prompt_echoes_send(
            send,
            "[Image #3][Image #4]compare these"
        ));
        assert!(prompt_echoes_send(
            send,
            "[Image #3] [Image #4] compare these"
        ));
        // Fewer placeholders than path lines: Claude recognized only part of
        // the attachment set, so the correlation stays conservative and refuses.
        assert!(!prompt_echoes_send(send, "[Image #3]compare these"));
    }

    #[test]
    fn a_send_that_is_only_an_attachment_path_matches_the_bare_placeholder() {
        assert!(prompt_echoes_send(
            "/home/dev/pictures/diagram.png",
            "[Image #1]"
        ));
    }

    #[test]
    fn attachment_matching_refuses_lines_that_are_not_image_paths() {
        // Relative path, no image extension, and prose starting with a slash
        // are all left in the body, so nothing is stripped and the exact-match
        // rule decides (it disagrees).
        for send in [
            "look\npictures/diagram.png",
            "look\n/home/dev/notes.txt",
            "look\n/home/dev is where it lives.png",
        ] {
            assert!(
                !prompt_echoes_send(send, "[Image #1]look"),
                "expected {send:?} to carry no attachment path line"
            );
        }
        // A prompt with no placeholder never takes the attachment path at all.
        assert!(!prompt_echoes_send("look\n/home/dev/diagram.png", "look"));
    }

    #[test]
    fn image_placeholder_stripping_requires_a_well_formed_number() {
        assert_eq!(strip_image_placeholder("[Image #12]rest"), Some("rest"));
        assert_eq!(strip_image_placeholder("[Image #]rest"), None);
        assert_eq!(strip_image_placeholder("[Image #1"), None);
        assert_eq!(strip_image_placeholder("[Image #1x]rest"), None);
        assert_eq!(strip_image_placeholder("no placeholder"), None);
    }

    #[test]
    fn unknown_command_notice_is_detected_by_prefix() {
        assert!(is_unknown_command_notice("Unknown command: /review-pr"));
        // A leading-token check: a human prompt merely mentioning the phrase
        // mid-line is not the notice.
        assert!(!is_unknown_command_notice(
            "why did Unknown command: appear?"
        ));
        assert!(!is_unknown_command_notice("a normal prompt"));
        assert!(!is_unknown_command_notice(""));
    }

    #[test]
    fn unknown_command_is_extracted_from_the_notice() {
        assert_eq!(
            unknown_command_from_notice("Unknown command: /review-pr"),
            Some("/review-pr")
        );
        // The token is trimmed even with irregular spacing after the colon.
        assert_eq!(
            unknown_command_from_notice("Unknown command:   /foo  "),
            Some("/foo")
        );
        // Not a notice, or a notice naming no command, yields nothing.
        assert_eq!(unknown_command_from_notice("a normal prompt"), None);
        assert_eq!(unknown_command_from_notice("Unknown command:"), None);
        assert_eq!(unknown_command_from_notice("Unknown command:   "), None);
    }

    #[test]
    fn launches_in_background_reads_the_top_level_flag() {
        assert!(launches_in_background(
            "Bash",
            &serde_json::json!({
                "command": "long-running",
                "run_in_background": true,
            }),
        ));
        assert!(!launches_in_background(
            "Bash",
            &serde_json::json!({
                "command": "ls",
                "run_in_background": false,
            }),
        ));
        // For a Bash call (or any non-subagent tool), absent key, wrong type,
        // and non-object inputs are all "foreground".
        assert!(!launches_in_background(
            "Bash",
            &serde_json::json!({ "command": "ls" }),
        ));
        assert!(!launches_in_background(
            "Bash",
            &serde_json::json!({ "run_in_background": "true" }),
        ));
        assert!(!launches_in_background("Bash", &serde_json::Value::Null));
    }

    #[test]
    fn launches_in_background_defaults_to_true_for_modern_agent_input() {
        // Modern Claude Code dropped the `run_in_background` parameter from the
        // `Agent`/`Task` tool schema and made these calls async by default. A
        // tool_use input that does not mention the key must be treated as
        // background, otherwise the running-subagent indicator clears as soon
        // as the immediate `PostToolUse(Agent)` fires.
        let modern_agent_input = serde_json::json!({
            "subagent_type": "general-purpose",
            "description": "Run ls and count entries",
            "prompt": "…",
        });
        assert!(launches_in_background("Agent", &modern_agent_input));
        assert!(launches_in_background("Task", &modern_agent_input));
        // A non-object input degrades to background for an Agent/Task call too:
        // the key is absent either way.
        assert!(launches_in_background("Agent", &serde_json::Value::Null));
    }

    #[test]
    fn launches_in_background_respects_explicit_false_for_agent() {
        // A caller that still passes `run_in_background: false` (e.g. an older
        // Claude Code, or a test fixture) keeps the foreground semantics it
        // asked for. Forward compatibility: if a future Claude reintroduces an
        // explicit foreground flag for Agent/Task, the predicate honours it.
        let explicit_foreground = serde_json::json!({
            "subagent_type": "general-purpose",
            "run_in_background": false,
        });
        assert!(!launches_in_background("Agent", &explicit_foreground));
        assert!(!launches_in_background("Task", &explicit_foreground));
        // An explicit `true` is honoured for Agent/Task as well, of course.
        assert!(launches_in_background(
            "Agent",
            &serde_json::json!({ "run_in_background": true }),
        ));
    }
}
