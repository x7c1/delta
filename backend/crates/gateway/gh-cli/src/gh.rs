//! [`Gh`]: the concrete [`GhCli`].

use async_trait::async_trait;
use chrono::{Duration, NaiveDate, Utc};
use tokio::process::Command;
use tokio::sync::OnceCell;

use delta_usecase::{GhCli, PullRequest, PullRequestLens};

use crate::error::Error;
use crate::parse::parse_search_response;

/// Max rows we ask gh to return per lens. 50 lines is plenty to scroll
/// through in the picker without inflating each call's payload.
const SEARCH_LIMIT: i64 = 50;

/// How far back (in days) the PR-search cutoff reaches. Today minus this
/// many days becomes the `updated:>=YYYY-MM-DD` floor on every lens, so
/// stale years-old PRs do not pollute the picker. Hardcoded for now; a
/// future settings screen may expose it.
const PR_FRESHNESS_DAYS: i64 = 365;

/// GraphQL the gateway runs through `gh api graphql` to fetch a PR list
/// for one lens.
///
/// We use the `search` API directly (rather than `gh search prs`)
/// because the CLI's `--json` projection for that subcommand does not
/// expose `headRefName` / `headRepository` — fields the PR tab needs to
/// pre-fill the composer's branch. The variable is named `$q` (rather
/// than `$query`) to avoid colliding with `gh api graphql`'s own
/// `query` parameter, which carries the GraphQL document itself; a
/// reused name makes gh reject the call with "unexpected override
/// existing field".
const SEARCH_GRAPHQL: &str = r#"
query($q: String!, $first: Int!) {
  search(query: $q, type: ISSUE, first: $first) {
    nodes {
      __typename
      ... on PullRequest {
        number
        title
        url
        isDraft
        updatedAt
        headRefName
        author { login }
        repository { nameWithOwner }
        headRepository { nameWithOwner }
      }
    }
  }
}
"#;

/// Drives the `gh` CLI for the new-session PR tab.
///
/// `is_authenticated` shells out to `gh auth status` on the first call
/// and caches the result for the rest of the server's lifetime — once
/// the host is set up the answer effectively does not change, and the
/// PR tab opens cheaply on a host with `gh` configured. Per-lens
/// search calls are not cached here (the use case owns that
/// memoisation).
#[derive(Debug, Default)]
pub struct Gh {
    /// Memoises the `gh auth status` answer. Async-locked so a
    /// concurrent burst of PR-tab opens only spawns one subprocess.
    auth_status: OnceCell<bool>,
}

impl Gh {
    /// Build a fresh gateway. The first `is_authenticated` call lazily
    /// runs `gh auth status` and memoises the result.
    pub fn new() -> Self {
        Self {
            auth_status: OnceCell::new(),
        }
    }

    /// Resolve the auth-status cache, populating it on first miss.
    async fn check_auth(&self) -> bool {
        *self
            .auth_status
            .get_or_init(|| async { run_auth_status().await })
            .await
    }
}

#[async_trait]
impl GhCli for Gh {
    async fn is_authenticated(&self) -> bool {
        self.check_auth().await
    }

    async fn search_prs(&self, lens: PullRequestLens) -> delta_usecase::Result<Vec<PullRequest>> {
        // `gh search prs --json` does not expose `headRefName` or
        // `headRepository` — fields the PR tab needs to pre-fill the
        // composer. Drive the GitHub search API directly via
        // `gh api graphql`, which can return those fields in one call.
        // `-F` for typed variables (the integer `first`); `-f` for
        // string variables (the query and the GraphQL document body).
        let first_arg = format!("first={SEARCH_LIMIT}");
        // Compute the freshness floor at call time so the cutoff slides with
        // the calendar. The pure builder takes the date as an argument so it
        // stays testable.
        let cutoff = freshness_cutoff(Utc::now().date_naive());
        let search_query = search_query_for(lens, cutoff);
        let q_arg = format!("q={search_query}");
        let doc_arg = format!("query={SEARCH_GRAPHQL}");
        let output = Command::new("gh")
            .args([
                "api", "graphql", "-F", &first_arg, "-f", &q_arg, "-f", &doc_arg,
            ])
            .output()
            .await
            .map_err(Error::from)?;
        if !output.status.success() {
            return Err(Error::Command {
                command: format!("gh api graphql ({lens})"),
                status: output.status.to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            }
            .into());
        }
        parse_search_response(&output.stdout).map_err(Into::into)
    }

    async fn clone_repo(
        &self,
        owner: &str,
        name: &str,
        destination: &str,
    ) -> delta_usecase::Result<()> {
        // `gh repo clone <owner>/<name> <dir>` rather than `git clone <url>`:
        // gh resolves the host and supplies the authenticated credentials, so a
        // private repository the account can see clones without Delta ever
        // handling a token. `destination` is always an absolute path derived
        // from a registered clone root, so it can never be mistaken for a flag.
        let slug = format!("{owner}/{name}");
        let output = Command::new("gh")
            .args(["repo", "clone", &slug, destination])
            .output()
            .await
            .map_err(Error::from)?;
        if !output.status.success() {
            return Err(Error::Command {
                command: format!("gh repo clone {slug}"),
                status: output.status.to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            }
            .into());
        }
        Ok(())
    }
}

/// Build the GitHub-search query string the GraphQL `search` resolver
/// expects for the named lens.
///
/// Mirrors `gh search prs`'s flag-to-qualifier translation: each lens
/// is the same set of qualifiers, just expressed inline. `cutoff_date`
/// becomes an `updated:>=YYYY-MM-DD` floor so stale PRs (a year or more
/// without an update) are filtered out at the source.
fn search_query_for(lens: PullRequestLens, cutoff_date: NaiveDate) -> String {
    // GitHub search expects `updated:>=YYYY-MM-DD`. `NaiveDate`'s `Display`
    // already renders ISO-8601 (`YYYY-MM-DD`), which is the format the search
    // qualifier requires.
    let updated = format!("updated:>={cutoff_date}");
    match lens {
        // Open PRs that requested my review and are NOT drafts.
        PullRequestLens::Reviewer => {
            format!("is:pr is:open review-requested:@me -draft:true {updated} sort:updated-desc")
        }
        // Open PRs I authored (drafts included).
        PullRequestLens::Author => {
            format!("is:pr is:open author:@me {updated} sort:updated-desc")
        }
    }
}

/// Compute the `updated:>=…` floor for a given reference date by stepping
/// back [`PR_FRESHNESS_DAYS`] days. Pulled out so the call site stays a
/// one-liner and tests can pass a fixed `today` without going through
/// `Utc::now`.
fn freshness_cutoff(today: NaiveDate) -> NaiveDate {
    today - Duration::days(PR_FRESHNESS_DAYS)
}

/// Run `gh auth status` and report whether gh considers itself
/// authenticated.
///
/// A missing binary (`NotFound`) and any other I/O error both collapse
/// to `false` so a host without gh installed still answers the
/// availability question rather than erroring — the use case represents
/// that as "PR tab disabled" rather than as a 5xx.
async fn run_auth_status() -> bool {
    match Command::new("gh").args(["auth", "status"]).output().await {
        Ok(output) => output.status.success(),
        Err(err) => {
            tracing::debug!(error = %err, "gh auth status failed; treating gh as unavailable");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed reference date used across the query-builder tests so
    /// assertions do not depend on the wall clock.
    fn fixed_today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, 24).expect("valid date")
    }

    #[test]
    fn freshness_cutoff_steps_back_one_year() {
        // 2026 is not a leap year; stepping back 365 days lands one
        // calendar year earlier on the same day.
        let cutoff = freshness_cutoff(fixed_today());
        assert_eq!(
            cutoff,
            NaiveDate::from_ymd_opt(2025, 6, 24).expect("valid date"),
        );
    }

    #[test]
    fn reviewer_query_includes_the_updated_floor() {
        let cutoff = freshness_cutoff(fixed_today());
        let query = search_query_for(PullRequestLens::Reviewer, cutoff);
        assert_eq!(
            query,
            "is:pr is:open review-requested:@me -draft:true \
             updated:>=2025-06-24 sort:updated-desc",
        );
    }

    #[test]
    fn author_query_includes_the_updated_floor() {
        let cutoff = freshness_cutoff(fixed_today());
        let query = search_query_for(PullRequestLens::Author, cutoff);
        assert_eq!(
            query,
            "is:pr is:open author:@me updated:>=2025-06-24 sort:updated-desc",
        );
    }

    #[test]
    fn cutoff_renders_as_iso_8601_in_the_query() {
        // A January-1st cutoff exercises zero-padding on month and day,
        // which the `updated:>=…` qualifier requires.
        let cutoff = NaiveDate::from_ymd_opt(2025, 1, 1).expect("valid date");
        let query = search_query_for(PullRequestLens::Author, cutoff);
        assert!(query.contains("updated:>=2025-01-01"), "got: {query}");
    }
}
