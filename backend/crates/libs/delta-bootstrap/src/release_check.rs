//! Wiring the release check to the configured feed.

use std::sync::Arc;

use delta_usecase::{ReleaseCheck, ReleaseFeed};
use release_feed::GithubReleaseFeed;

use crate::Config;

/// The release check `config` asks for, comparing against `current_version`
/// (the server's `CARGO_PKG_VERSION`).
///
/// Turned off when [`Config::release_feed_url`] is `None`. Building the HTTPS
/// client happens here, never on a request; when it fails (the platform trust
/// store cannot be set up) the check is turned off with a `warn`, since a
/// missing update notice must never stop the server.
pub fn release_check(config: &Config, current_version: &str) -> ReleaseCheck {
    let Some(url) = &config.release_feed_url else {
        tracing::info!("the release check is turned off (DELTA_RELEASE_FEED_URL is empty)");
        return ReleaseCheck::disabled();
    };
    match GithubReleaseFeed::new(url.clone()) {
        Ok(feed) => ReleaseCheck::new(Arc::new(feed) as Arc<dyn ReleaseFeed>, current_version),
        Err(err) => {
            tracing::warn!(error = %err, "the release check is turned off");
            ReleaseCheck::disabled()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::testing::test_config;

    #[test]
    fn no_feed_url_turns_the_check_off() {
        let dir = tempfile::tempdir().unwrap();
        let config = test_config(&dir);
        assert_eq!(config.release_feed_url, None);
        assert!(!release_check(&config, "0.5.0").is_enabled());
    }

    #[test]
    fn a_feed_url_turns_the_check_on() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            release_feed_url: Some(crate::DEFAULT_RELEASE_FEED_URL.to_owned()),
            ..test_config(&dir)
        };
        assert!(release_check(&config, "0.5.0").is_enabled());
    }
}
