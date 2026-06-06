//! The `content` field of a transcript message.

use serde::Deserialize;

use delta_model::ContentBlock;

/// Content is either a bare string (user prompts) or an array of typed blocks.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum RawContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}
