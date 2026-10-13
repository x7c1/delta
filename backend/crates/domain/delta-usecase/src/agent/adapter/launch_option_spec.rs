//! One selected launch option as the neutral `(name, value?)` pair.

/// One launch option the user selected for this session, resolved from the
/// registry into the neutral `(name, value?)` pair the registry stores.
///
/// The pair is deliberately **not** rendered here: what a `name` means is a
/// provider concern, and rendering it is the adapter's job. Claude reads the
/// pair as a CLI flag and its argument (`--model opus`, see [`Self::to_argv`]);
/// Codex reads it as a `thread/start` field name and its value (`model` →
/// `"gpt-5.6-sol"`). Keeping the pair neutral is what lets the core hand every
/// provider the same list instead of branching on which provider it is talking
/// to.
///
/// Values arrive already resolved (a leading `~` is expanded by the core, since
/// no shell ever runs over a launch-option value for any provider), so an
/// adapter renders them verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOptionSpec {
    /// What the option is called — a CLI flag for an argv-launched provider
    /// (`--permission-mode`), a request field name for a structured one
    /// (`sandbox`).
    pub name: String,
    /// The option's argument/value; `None` for a valueless option.
    pub value: Option<String>,
}

impl LaunchOptionSpec {
    /// Render this option as argv tokens, the shape a command-line-launched
    /// provider needs: the name, followed by the value when there is one (a
    /// valueless option contributes only its name).
    pub fn to_argv(&self) -> Vec<String> {
        match &self.value {
            Some(value) => vec![self.name.clone(), value.clone()],
            None => vec![self.name.clone()],
        }
    }
}
