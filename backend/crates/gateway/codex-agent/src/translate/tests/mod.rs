//! Translation tests, split by the frames they cover.

mod approval;
mod file_change;
mod item;
mod turn;
mod usage;

use super::*;
use delta_usecase::RateLimitWindow;
use serde_json::json;

fn notification(method: &str, params: Value) -> Notification {
    Notification {
        method: method.to_owned(),
        params,
    }
}
