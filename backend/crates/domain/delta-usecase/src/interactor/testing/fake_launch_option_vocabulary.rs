//! A [`LaunchOptionVocabulary`] for the domain's own tests.

use crate::agent::{AgentProvider, LaunchOptionCardinality, LaunchOptionVocabulary};

/// The name [`FakeLaunchOptionVocabulary`] classifies as dangerous.
pub(crate) const DANGEROUS_NAME: &str = "--dangerously-skip-permissions";

/// The names [`FakeLaunchOptionVocabulary`] classifies as single-valued: one
/// argv-style flag and one field-style name, so both spawn paths have a choice
/// group to select from.
const SINGLE_VALUED_NAMES: &[&str] = &["--model", "model"];

/// A stub vocabulary: [`DANGEROUS_NAME`] is dangerous, the
/// [`SINGLE_VALUED_NAMES`] take one value, and everything else is benign and
/// repeatable — for every provider alike.
///
/// The real vocabulary lives in the gateway adapters (and is tested there), so
/// what these tests need is only *a* dangerous option and *a* single-valued
/// name — a stub keeps the use-case rules under test without dragging Claude's
/// or Codex's spellings into the domain.
pub(crate) struct FakeLaunchOptionVocabulary;

impl LaunchOptionVocabulary for FakeLaunchOptionVocabulary {
    fn is_dangerous(&self, _provider: AgentProvider, name: &str, _value: Option<&str>) -> bool {
        name == DANGEROUS_NAME
    }

    fn cardinality(&self, _provider: AgentProvider, name: &str) -> LaunchOptionCardinality {
        if SINGLE_VALUED_NAMES.contains(&name) {
            LaunchOptionCardinality::Single
        } else {
            LaunchOptionCardinality::Multiple
        }
    }
}
