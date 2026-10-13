//! [`WireSessionEvent`] serialization tests, split by the events they cover.

mod event_kinds;
mod lifecycle;
mod permissions;
mod questions;
mod repository_clone;
mod sends;
mod spawn_failed;
mod status;
mod subagents;
mod turns;

use super::*;
use crate::file_change::WireFileChange;
use crate::file_change_kind::WireFileChangeKind;
use delta_usecase::{RateLimitWindow, StatusSnapshot};

use delta_model::{AgentProvider, MessageUuid, SessionId, ThreadId};

fn json(event: &WireSessionEvent) -> serde_json::Value {
    serde_json::to_value(event).unwrap()
}
