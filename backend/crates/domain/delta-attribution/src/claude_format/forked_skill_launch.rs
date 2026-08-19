//! The `<forked-skill-launch>` element and the launch payload it carries.

use super::json_string_field;

/// Opening tag of the element Claude Code writes when a slash command forks
/// its skill into a **background agent** (e.g. `/review-pr`, recorded as
/// `/example:review-pr`). It rides the same `type: "system"` /
/// `subtype: "local_command"` line as the command's `<local-command-stdout>`
/// ("Running in the background as @example-review-pr") and carries a JSON
/// body: `{"agentId":…,"skillName":…,"description":…}`.
///
/// This element is the ONLY signal such a launch produces in the parent
/// transcript: the CLI harness launches the forked skill itself, so — unlike an
/// `Agent`/`Task` the model calls — no `tool_use` block is ever written.
const FORKED_SKILL_LAUNCH_OPEN: &str = "<forked-skill-launch>";

/// Closing tag of [`FORKED_SKILL_LAUNCH_OPEN`].
const FORKED_SKILL_LAUNCH_CLOSE: &str = "</forked-skill-launch>";

/// Namespace for the synthetic `tool_use_id` Delta mints for a forked skill.
/// A forked skill has no `tool_use` block and therefore no real `toolu_...`
/// id, so its correlation key is derived from the payload's `agentId` behind
/// this prefix — which no genuine tool_use id can collide with.
const FORKED_SKILL_TOOL_USE_PREFIX: &str = "forked-skill:";

/// The payload of a `<forked-skill-launch>` element: a background agent the CLI
/// harness started for a slash command's skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkedSkillLaunch {
    /// The background-task identifier of the forked agent. Equals the
    /// `<task-id>` element of the `<task-notification>` that later reports its
    /// completion, which is how the completion correlates back to this launch.
    pub agent_id: String,
    /// The fully-qualified skill name (e.g. `example:review-pr`), used as
    /// the running indicator's `subagent_type`. `None` when the payload omits
    /// it or leaves it empty.
    pub skill_name: Option<String>,
    /// The launch description (e.g. `/example:review-pr`), displayed next
    /// to the indicator. `None` when the payload omits it or leaves it empty.
    pub description: Option<String>,
}

impl ForkedSkillLaunch {
    /// The synthetic `tool_use_id` this launch is tracked under: the payload's
    /// `agentId` behind the [`FORKED_SKILL_TOOL_USE_PREFIX`] namespace. A
    /// forked skill writes no `tool_use` block, so it has no real id — but the
    /// running-subagent indicator, the persisted launch row, and the completion
    /// correlation are all keyed by `tool_use_id`, so one is minted here.
    pub fn tool_use_id(&self) -> String {
        format!("{FORKED_SKILL_TOOL_USE_PREFIX}{}", self.agent_id)
    }
}

/// Whether a (trimmed) line text carries a `<forked-skill-launch>` element at
/// all. Separate from [`forked_skill_launch`] so the caller can tell "this line
/// is not a forked-skill launch" (say nothing) from "it is one, but its body
/// did not parse" (worth logging, so a Claude Code format change surfaces in
/// the logs instead of as a silently dark running indicator).
pub fn has_forked_skill_launch(text: &str) -> bool {
    text.contains(FORKED_SKILL_LAUNCH_OPEN)
}

/// Parse the `<forked-skill-launch>` element a line carries, if any.
///
/// Returns `None` when the element is absent, unterminated, its body is not
/// JSON, or the body names no non-empty `agentId` — the id is the correlation
/// key for the whole lifecycle, so a payload without one is unusable.
/// `skillName` and `description` are optional display fields.
pub fn forked_skill_launch(text: &str) -> Option<ForkedSkillLaunch> {
    let start = text.find(FORKED_SKILL_LAUNCH_OPEN)? + FORKED_SKILL_LAUNCH_OPEN.len();
    let rest = &text[start..];
    let end = rest.find(FORKED_SKILL_LAUNCH_CLOSE)?;
    let payload: serde_json::Value = serde_json::from_str(rest[..end].trim()).ok()?;
    let field = |key: &str| json_string_field(&payload, key).map(str::to_owned);
    Some(ForkedSkillLaunch {
        agent_id: field("agentId")?,
        skill_name: field("skillName"),
        description: field("description"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The line Claude Code 2.1.234 writes for a `/…` command that forks its
    /// skill into a background agent: one `type: "system"` /
    /// `subtype: "local_command"` line whose top-level content carries BOTH
    /// the command's stdout and the launch element. Taken verbatim from a real
    /// transcript except for the skill name, which is a placeholder.
    const REAL_FORKED_SKILL_LINE: &str = "<local-command-stdout>Running in the background as \
         @example-review-pr</local-command-stdout>\n\
         <forked-skill-launch>{\"agentId\":\"a7046b32df40e1b3e\",\
         \"skillName\":\"example:review-pr\",\
         \"description\":\"/example:review-pr\"}</forked-skill-launch>";

    #[test]
    fn a_forked_skill_launch_is_parsed_from_the_real_local_command_line() {
        assert!(has_forked_skill_launch(REAL_FORKED_SKILL_LINE));
        assert_eq!(
            forked_skill_launch(REAL_FORKED_SKILL_LINE),
            Some(ForkedSkillLaunch {
                agent_id: "a7046b32df40e1b3e".into(),
                skill_name: Some("example:review-pr".into()),
                description: Some("/example:review-pr".into()),
            })
        );
        // The synthetic tool_use id is namespaced, so it can never collide
        // with a genuine `toolu_...` id.
        assert_eq!(
            forked_skill_launch(REAL_FORKED_SKILL_LINE)
                .expect("the payload parses")
                .tool_use_id(),
            "forked-skill:a7046b32df40e1b3e"
        );
    }

    #[test]
    fn a_line_without_the_element_carries_no_forked_skill_launch() {
        // A plain local-command output line (a slash command that forks
        // nothing) and an ordinary prompt both yield nothing.
        for text in [
            "<local-command-stdout>PENDING review created.</local-command-stdout>",
            "a normal prompt",
            "",
        ] {
            assert!(!has_forked_skill_launch(text), "{text:?}");
            assert_eq!(forked_skill_launch(text), None, "{text:?}");
        }
    }

    #[test]
    fn a_forked_skill_element_with_an_unusable_body_parses_to_none() {
        // Present but unusable: unterminated, unparsable JSON, JSON that is
        // not an object, no `agentId`, and an empty `agentId`. Each is
        // DETECTED (so the caller logs it) but yields no launch.
        for body in [
            "<forked-skill-launch>{\"agentId\":\"a1\"}",
            "<forked-skill-launch>not json</forked-skill-launch>",
            "<forked-skill-launch>[1,2]</forked-skill-launch>",
            "<forked-skill-launch>{\"skillName\":\"s\"}</forked-skill-launch>",
            "<forked-skill-launch>{\"agentId\":\"\"}</forked-skill-launch>",
        ] {
            assert!(has_forked_skill_launch(body), "{body:?}");
            assert_eq!(forked_skill_launch(body), None, "{body:?}");
        }
    }

    #[test]
    fn a_forked_skill_payload_without_display_fields_still_parses() {
        // `skillName` / `description` are display-only: a payload missing them
        // must still light the indicator, just without labels.
        assert_eq!(
            forked_skill_launch("<forked-skill-launch>{\"agentId\":\"a1\"}</forked-skill-launch>"),
            Some(ForkedSkillLaunch {
                agent_id: "a1".into(),
                skill_name: None,
                description: None,
            })
        );
    }
}
