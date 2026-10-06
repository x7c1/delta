//! Which sessions a bulk removal takes.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use delta_model::SessionStatus;

use super::PruneStatus;

/// Seconds in a day, for turning "older than N days" into a cut-off.
const SECS_PER_DAY: u64 = 86_400;

/// The sessions a bulk removal takes: those whose most recent activity is at
/// least `older_than_days` days old and that ended one of the `statuses`
/// ways.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneCriteria {
    /// How many days ago the session's most recent activity must be, at least.
    /// Zero takes every closed session regardless of age.
    pub older_than_days: u32,
    /// How the session ended. Empty matches nothing.
    pub statuses: Vec<PruneStatus>,
}

impl PruneCriteria {
    /// The latest recency a session may have to be old enough at `now`, as an
    /// ISO-8601 UTC timestamp comparable with the stored ones.
    pub fn cutoff(&self, now: SystemTime) -> String {
        let age = Duration::from_secs(u64::from(self.older_than_days) * SECS_PER_DAY);
        let secs = now
            .checked_sub(age)
            .and_then(|cutoff| cutoff.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |since_epoch| since_epoch.as_secs());
        delta_model::iso8601_utc(secs)
    }

    /// The row statuses [`Self::statuses`] cover, each once.
    pub fn row_statuses(&self) -> Vec<SessionStatus> {
        let mut statuses: Vec<SessionStatus> = Vec::new();
        for status in self
            .statuses
            .iter()
            .flat_map(|choice| choice.row_statuses())
        {
            if !statuses.contains(status) {
                statuses.push(*status);
            }
        }
        statuses
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cutoff_is_that_many_days_before_now() {
        let now = UNIX_EPOCH + Duration::from_secs(1_609_459_200); // 2021-01-01
        let criteria = PruneCriteria {
            older_than_days: 31,
            statuses: PruneStatus::ALL.to_vec(),
        };
        assert_eq!(criteria.cutoff(now), "2020-12-01T00:00:00Z");
        let today = PruneCriteria {
            older_than_days: 0,
            ..criteria
        };
        assert_eq!(today.cutoff(now), "2021-01-01T00:00:00Z");
    }

    #[test]
    fn a_cutoff_before_the_epoch_clamps_to_it() {
        let criteria = PruneCriteria {
            older_than_days: 10,
            statuses: Vec::new(),
        };
        assert_eq!(criteria.cutoff(UNIX_EPOCH), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn ended_covers_active_and_ended_rows_and_duplicates_collapse() {
        let criteria = PruneCriteria {
            older_than_days: 0,
            statuses: vec![PruneStatus::Ended, PruneStatus::Failed, PruneStatus::Ended],
        };
        assert_eq!(
            criteria.row_statuses(),
            vec![
                SessionStatus::Active,
                SessionStatus::Ended,
                SessionStatus::Failed
            ]
        );
    }
}
