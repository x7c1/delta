//! Request body for `POST /api/sessions/prune`.

use delta_usecase::{PruneCriteria, PruneStatus};
use serde::Deserialize;
use ts_rs::TS;

use super::WirePruneStatus;

/// Request body for `POST /api/sessions/prune`: remove every closed session
/// whose most recent activity is at least `older_than_days` days ago and that
/// ended one of the `statuses` ways.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(rename = "PruneSessionsRequest")]
pub struct WirePruneSessionsRequest {
    /// Days since the session's most recent activity (its last message, or
    /// its creation when it has none), at least. `0` takes every closed
    /// session regardless of age.
    pub older_than_days: u32,
    /// How the session ended. Absent means both; an empty list matches
    /// nothing.
    #[serde(default)]
    #[ts(optional)]
    pub statuses: Option<Vec<WirePruneStatus>>,
}

impl From<WirePruneSessionsRequest> for PruneCriteria {
    fn from(request: WirePruneSessionsRequest) -> Self {
        PruneCriteria {
            older_than_days: request.older_than_days,
            statuses: match request.statuses {
                Some(statuses) => statuses.into_iter().map(PruneStatus::from).collect(),
                None => PruneStatus::ALL.to_vec(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_default_to_both() {
        let request: WirePruneSessionsRequest =
            serde_json::from_str(r#"{ "older_than_days": 30 }"#).unwrap();
        let criteria = PruneCriteria::from(request);
        assert_eq!(criteria.older_than_days, 30);
        assert_eq!(criteria.statuses, PruneStatus::ALL.to_vec());
    }

    #[test]
    fn named_statuses_are_kept_and_a_negative_age_is_rejected() {
        let request: WirePruneSessionsRequest =
            serde_json::from_str(r#"{ "older_than_days": 0, "statuses": ["failed"] }"#).unwrap();
        assert_eq!(
            PruneCriteria::from(request).statuses,
            vec![PruneStatus::Failed]
        );
        assert!(
            serde_json::from_str::<WirePruneSessionsRequest>(r#"{ "older_than_days": -1 }"#)
                .is_err()
        );
        assert!(serde_json::from_str::<WirePruneSessionsRequest>(r#"{}"#).is_err());
    }
}
