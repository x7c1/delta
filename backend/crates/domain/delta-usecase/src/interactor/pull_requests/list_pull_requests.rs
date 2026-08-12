//! `list_pull_requests`: drive the gh CLI for one lens and stamp each row
//! with whether Delta has a local clone of its repository.

use std::collections::HashSet;
use std::time::Instant;

use crate::error::Result;
use crate::interactor::{InteractorCore, PrSearchCacheEntry, PR_SEARCH_CACHE_TTL};
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::pull_request::{PullRequest, PullRequestLens, PullRequestList};

/// The host segment GitHub uses for the canonical `identity_key`. `gh`
/// only ever talks to GitHub, so PR identity keys always sit under this
/// host — bundling them with on-disk clones whose `origin` is on
/// `github.com`.
const GITHUB_HOST: &str = "github.com";

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// List the open pull requests for `lens`, each stamped with whether
    /// Delta knows a local clone of its repository.
    ///
    /// `gh` availability is checked first: when `gh` is not installed or
    /// `gh auth status` fails the list is empty and `gh_available` is
    /// `false`, so the endpoint never 5xx's on missing tooling. The
    /// gateway's actual search call — a GitHub search query issued through
    /// `gh api graphql` — is memoised per-lens for [`PR_SEARCH_CACHE_TTL`]
    /// so flipping between the panel's lenses (or remounting it) does not
    /// re-shell on every focus change.
    ///
    /// `has_local_clone` on each row is derived by joining the gh result
    /// against `list_repositories`: a row's PR repository is considered
    /// "registered" when at least one repository's `identity_key` matches
    /// `github.com/<owner>/<name>`. (`gh` is GitHub-only, so the host is
    /// always `github.com`.)
    pub async fn list_pull_requests(&self, lens: PullRequestLens) -> Result<PullRequestList> {
        if !self.gh_cli.is_authenticated().await {
            return Ok(PullRequestList {
                gh_available: false,
                pull_requests: Vec::new(),
            });
        }

        let raw = self.cached_pr_search(lens).await?;
        let registered = self.registered_github_identity_keys().await?;
        let pull_requests = raw
            .into_iter()
            .map(|pr| {
                let key = format!("{GITHUB_HOST}/{}/{}", pr.repo_owner, pr.repo_name);
                let has_local_clone = registered.contains(&key);
                PullRequest {
                    has_local_clone,
                    ..pr
                }
            })
            .collect();

        Ok(PullRequestList {
            gh_available: true,
            pull_requests,
        })
    }

    /// Look up (and memoise) the gh search result for `lens`.
    ///
    /// Each lens caches independently — the reviewer and author result
    /// sets are largely disjoint, and a stale reviewer list should not
    /// block a fresh author refresh. A miss shells out to the gh driver
    /// and stamps the entry with the wall-clock at which it was fetched;
    /// further hits inside [`PR_SEARCH_CACHE_TTL`] reuse the cached vec.
    async fn cached_pr_search(&self, lens: PullRequestLens) -> Result<Vec<PullRequest>> {
        let now = Instant::now();
        {
            let cache = self.pr_search_cache.lock().await;
            if let Some(entry) = cache.get(&lens) {
                if now.duration_since(entry.fetched_at) < PR_SEARCH_CACHE_TTL {
                    return Ok(entry.pull_requests.clone());
                }
            }
        }
        let fresh = self.gh_cli.search_prs(lens).await?;
        let mut cache = self.pr_search_cache.lock().await;
        cache.insert(
            lens,
            PrSearchCacheEntry {
                fetched_at: now,
                pull_requests: fresh.clone(),
            },
        );
        Ok(fresh)
    }

    /// The set of `identity_key`s that look like a GitHub repo and have at
    /// least one registered local clone.
    ///
    /// Built by running the Repository tab's aggregation and keeping the
    /// `github.com/...`-shaped identity keys. Path-keyed entries (those
    /// whose `origin` was unset) cannot match a `gh`-sourced row by
    /// construction — `gh` answers only with GitHub repos — so they are
    /// filtered out.
    async fn registered_github_identity_keys(&self) -> Result<HashSet<String>> {
        let repositories = self.list_repositories().await?;
        Ok(repositories
            .into_iter()
            .filter(|repo| is_github_identity_key(&repo.identity_key))
            .map(|repo| repo.identity_key)
            .collect())
    }
}

/// True when `key` looks like the normalised origin key for a github.com
/// repository — `github.com/<owner>/<name>` (possibly deeper). Path-keyed
/// entries (which the identity-key normaliser preserves verbatim) start
/// with `/` and so never match.
fn is_github_identity_key(key: &str) -> bool {
    let mut segments = key.split('/');
    segments.next().is_some_and(|host| host == GITHUB_HOST)
        && segments.next().is_some_and(|owner| !owner.is_empty())
        && segments.next().is_some_and(|name| !name.is_empty())
}

#[cfg(test)]
mod identity_key_shape {
    use super::*;

    #[test]
    fn shapes_a_github_origin_key_matches() {
        assert!(is_github_identity_key("github.com/x7c1/delta"));
    }

    #[test]
    fn shapes_a_path_key_does_not_match() {
        // Path-keyed entries (origin unset) carry the absolute path
        // verbatim; they can never collide with a GitHub-shaped key.
        assert!(!is_github_identity_key("/home/dev/projects/scratch"));
    }

    #[test]
    fn shapes_a_non_github_host_does_not_match() {
        assert!(!is_github_identity_key("gitlab.com/x7c1/delta"));
    }

    #[test]
    fn shapes_a_partial_key_does_not_match() {
        // Just a host is not enough — `gh` always answers with an owner
        // and a name, so a bare host (or `host/owner`) cannot collide
        // with a PR's `github.com/<owner>/<name>` lookup.
        assert!(!is_github_identity_key("github.com"));
        assert!(!is_github_identity_key("github.com/x7c1"));
    }
}
