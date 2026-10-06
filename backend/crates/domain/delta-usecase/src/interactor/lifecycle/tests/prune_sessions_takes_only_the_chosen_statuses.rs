use std::time::{Duration, SystemTime};

use delta_model::SessionStatus;

use crate::interactor::testing::*;
use crate::{PruneCriteria, PruneStatus};

use super::removal_support::closed_session;

/// Choosing only failed launches leaves the sessions that ran, and choosing
/// nothing removes nothing.
#[tokio::test]
async fn prune_sessions_takes_only_the_chosen_statuses() {
    let now = SystemTime::now() + Duration::from_secs(10 * 365 * 86_400);
    let ix = interactor();
    let ran = closed_session(&ix, "ran", "/scratch/a", None, None).await;
    let failed = closed_session(&ix, "failed", "/scratch/b", None, None).await;
    ix.store()
        .set_session_status(&failed, SessionStatus::Failed);
    let only = |statuses: Vec<PruneStatus>| PruneCriteria {
        older_than_days: 0,
        statuses,
    };

    let nothing = ix.prune_sessions(&only(Vec::new()), now).await.unwrap();
    let failed_only = ix
        .prune_sessions(&only(vec![PruneStatus::Failed]), now)
        .await
        .unwrap();

    assert!(nothing.removed.is_empty() && nothing.skipped.is_empty());
    assert_eq!(failed_only.removed, vec![failed]);
    assert!(ix.store().session(&ran).await.unwrap().is_some());
}
