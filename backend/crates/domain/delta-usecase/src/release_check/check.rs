use super::{judge_release, ReleaseCheck, ReleaseCheckError};

impl ReleaseCheck {
    /// Ask the feed once and record the verdict.
    ///
    /// A failure logs one `warn` naming the cause and keeps the previous
    /// verdict; it is also returned, for the caller's tests. A turned-off check
    /// asks nothing and succeeds.
    pub async fn check(&self) -> Result<(), ReleaseCheckError> {
        let Some(feed) = &self.feed else {
            return Ok(());
        };
        let verdict = match feed.latest_release().await {
            Ok(release) => judge_release(&self.current, release),
            Err(err) => Err(err.into()),
        };
        match verdict {
            Ok(newer) => {
                match &newer {
                    Some(release) => tracing::info!(
                        version = %release.display_version(),
                        url = release.url(),
                        "a newer release of Delta is out"
                    ),
                    None => tracing::debug!("Delta is up to date with its newest release"),
                }
                *self.newer.lock().expect("release check mutex poisoned") = newer;
                Ok(())
            }
            Err(err) => {
                tracing::warn!(error = %with_causes(&err), "could not check for a newer release of Delta");
                Err(err)
            }
        }
    }
}

/// `err` followed by each of its causes, joined with `: `.
///
/// A feed failure's own `Display` names only its kind (reqwest's says just
/// "error sending request"); what a user needs in the log — DNS, a refused
/// connection, a timeout, a certificate the trust store rejects — is further
/// down the chain.
fn with_causes(err: &dyn std::error::Error) -> String {
    let mut message = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

#[cfg(test)]
mod tests {
    use super::super::testing::{check_with, release, ScriptedFeed};
    use super::super::RELEASE_PAGE_PREFIX;
    use super::*;
    use crate::ports::{PublishedRelease, ReleaseFeedError};

    fn newer_version(check: &ReleaseCheck) -> Option<String> {
        check.newer().map(|r| r.display_version())
    }

    #[test]
    fn a_failure_is_logged_with_its_whole_cause_chain() {
        let refused =
            std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "connection refused");
        let err = ReleaseCheckError::from(ReleaseFeedError::Request(Box::new(refused)));
        assert_eq!(
            with_causes(&err),
            "no answer arrived from the feed: connection refused"
        );
    }

    #[tokio::test]
    async fn a_newer_release_is_recorded_with_its_page() {
        let feed = ScriptedFeed::new(vec![Ok(release("v0.6.0"))]);
        let check = check_with(&feed);
        check.check().await.unwrap();
        let newer = check.newer().expect("newer");
        assert_eq!(newer.display_version(), "v0.6.0");
        assert_eq!(newer.url(), format!("{RELEASE_PAGE_PREFIX}tag/v0.6.0"));
    }

    #[tokio::test]
    async fn an_up_to_date_check_records_nothing_newer() {
        let feed = ScriptedFeed::new(vec![Ok(release("v0.5.0"))]);
        let check = check_with(&feed);
        check.check().await.unwrap();
        assert_eq!(check.newer(), None);
    }

    #[tokio::test]
    async fn every_failure_is_reported_and_records_nothing() {
        type Expect = fn(&ReleaseCheckError) -> bool;
        let failures: [(Result<PublishedRelease, ReleaseFeedError>, Expect); 5] = [
            (
                Err(ReleaseFeedError::Request("connection refused".into())),
                |err| matches!(err, ReleaseCheckError::Feed(ReleaseFeedError::Request(_))),
            ),
            (Err(ReleaseFeedError::Status(403)), |err| {
                matches!(err, ReleaseCheckError::Feed(ReleaseFeedError::Status(403)))
            }),
            (
                Err(ReleaseFeedError::Malformed("expected value".into())),
                |err| matches!(err, ReleaseCheckError::Feed(ReleaseFeedError::Malformed(_))),
            ),
            (
                Ok(PublishedRelease {
                    tag_name: "v0.6.0".into(),
                    html_url: "https://example.com/delta".into(),
                }),
                |err| matches!(err, ReleaseCheckError::Page(page) if page == "https://example.com/delta"),
            ),
            (
                Ok(release("nightly")),
                |err| matches!(err, ReleaseCheckError::Tag { tag, .. } if tag == "nightly"),
            ),
        ];
        for (answer, expected) in failures {
            let feed = ScriptedFeed::new(vec![answer]);
            let check = check_with(&feed);
            let err = check.check().await.unwrap_err();
            assert!(expected(&err), "{err:?}");
            assert_eq!(check.newer(), None);
        }
    }

    #[tokio::test]
    async fn a_failed_check_after_a_successful_one_keeps_the_earlier_newer() {
        let feed = ScriptedFeed::new(vec![
            Ok(release("v0.6.0")),
            Err(ReleaseFeedError::Request("dns error".into())),
            Err(ReleaseFeedError::Status(403)),
            Err(ReleaseFeedError::Malformed("eof".into())),
            Ok(PublishedRelease {
                tag_name: "v0.7.0".into(),
                html_url: "https://example.com/v0.7.0".into(),
            }),
        ]);
        let check = check_with(&feed);
        check.check().await.unwrap();
        for _ in 0..4 {
            assert!(check.check().await.is_err());
            assert_eq!(newer_version(&check), Some("v0.6.0".into()));
        }
    }

    #[tokio::test]
    async fn a_later_newer_release_replaces_the_earlier_one() {
        let feed = ScriptedFeed::new(vec![Ok(release("v0.6.0")), Ok(release("v0.7.0"))]);
        let check = check_with(&feed);
        check.check().await.unwrap();
        check.check().await.unwrap();
        assert_eq!(newer_version(&check), Some("v0.7.0".into()));
    }
}
