//! Test helpers shared by the release check's tests.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use super::{ReleaseCheck, RELEASE_PAGE_PREFIX};
use crate::ports::{PublishedRelease, ReleaseFeed, ReleaseFeedError};

/// A well-formed answer for `tag`, with its page under the pinned prefix.
pub(super) fn release(tag: &str) -> PublishedRelease {
    PublishedRelease {
        tag_name: tag.to_owned(),
        html_url: format!("{RELEASE_PAGE_PREFIX}tag/{tag}"),
        assets: Vec::new(),
    }
}

/// A feed that answers each call with the next scripted answer and counts
/// the calls.
pub(super) struct ScriptedFeed {
    answers: Mutex<Vec<Result<PublishedRelease, ReleaseFeedError>>>,
    calls: AtomicUsize,
}

impl ScriptedFeed {
    pub(super) fn new(answers: Vec<Result<PublishedRelease, ReleaseFeedError>>) -> Arc<Self> {
        let mut answers = answers;
        answers.reverse();
        Arc::new(Self {
            answers: Mutex::new(answers),
            calls: AtomicUsize::new(0),
        })
    }

    /// How many times the feed has been asked.
    pub(super) fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ReleaseFeed for ScriptedFeed {
    async fn latest_release(&self) -> Result<PublishedRelease, ReleaseFeedError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.answers
            .lock()
            .unwrap()
            .pop()
            .expect("no scripted answer left")
    }

    fn url(&self) -> &str {
        "https://feed.invalid/scripted"
    }
}

/// A check of a `0.5.0` build asking `feed`.
pub(super) fn check_with(feed: &Arc<ScriptedFeed>) -> ReleaseCheck {
    ReleaseCheck::new(Arc::clone(feed) as Arc<dyn ReleaseFeed>, "0.5.0")
}
