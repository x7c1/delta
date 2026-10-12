//! What the browser may offer next to the newer-release notice.

use delta_usecase::UpdateOffer;
use serde::Serialize;
use ts_rs::TS;

/// What the browser may offer next to the newer-release notice, decided by
/// who launched the server, where it was built, and whether the newer release
/// has an asset it would download.
///
/// - `update`: the Update action (`POST /api/latest-release/download`) — a
///   desktop app built by the release workflow, for a newer release with an
///   asset this platform may download.
/// - `rebuild`: a hint that this desktop app was built locally and is updated
///   by rebuilding it (`make desktop`).
/// - `none`: the notice and its link only — the CLI server, a desktop release
///   build whose HTTPS client could not be set up, or a newer release with no
///   asset this platform may download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "UpdateOffer")]
pub enum WireUpdateOffer {
    Update,
    Rebuild,
    None,
}

impl From<UpdateOffer> for WireUpdateOffer {
    fn from(offer: UpdateOffer) -> Self {
        match offer {
            UpdateOffer::Update => Self::Update,
            UpdateOffer::Rebuild => Self::Rebuild,
            UpdateOffer::None => Self::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_offer_serializes_in_snake_case() {
        for (offer, json) in [
            (WireUpdateOffer::Update, "update"),
            (WireUpdateOffer::Rebuild, "rebuild"),
            (WireUpdateOffer::None, "none"),
        ] {
            assert_eq!(
                serde_json::to_value(offer).unwrap(),
                serde_json::json!(json)
            );
        }
    }
}
