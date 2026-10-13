//! Which way one frame travelled. Wire twin of [`CommsDirection`].

use delta_usecase::CommsDirection;
use serde::Serialize;
use ts_rs::TS;

/// Which way one frame travelled. Wire twin of [`CommsDirection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "CommsDirection")]
pub enum WireCommsDirection {
    /// Delta → agent: a frame Delta wrote.
    ToAgent,
    /// Agent → Delta: a frame Delta read.
    FromAgent,
}

impl From<CommsDirection> for WireCommsDirection {
    fn from(direction: CommsDirection) -> Self {
        match direction {
            CommsDirection::ToAgent => WireCommsDirection::ToAgent,
            CommsDirection::FromAgent => WireCommsDirection::FromAgent,
        }
    }
}
