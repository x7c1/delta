//! Parse `gh api graphql` output (the PR-search response) into
//! [`PullRequest`] rows.
//!
//! The GraphQL query lives in `gh.rs`; the JSON shape it returns is
//! `{ data: { search: { nodes: [{...PullRequest fields}] } } }`. We
//! deserialise only the fields the PR tab consumes; extras are
//! tolerated so a future GraphQL change that adds siblings does not
//! break the parser.

use delta_usecase::PullRequest;
use serde::Deserialize;

use crate::error::{Error, Result};

/// The top-level shape of `gh api graphql` for our search query.
#[derive(Debug, Deserialize)]
struct SearchResponse {
    data: SearchData,
}

#[derive(Debug, Deserialize)]
struct SearchData {
    search: SearchEdges,
}

#[derive(Debug, Deserialize)]
struct SearchEdges {
    /// One node per matching item. The GraphQL search type is `ISSUE`
    /// and includes both Issues and PRs; we filter on `__typename` to
    /// keep just the PR variants. In practice the `is:pr` qualifier
    /// already pins this.
    nodes: Vec<SearchNode>,
}

/// One search hit. Non-PR nodes are tolerated — fields the GraphQL
/// `... on PullRequest` selection set returns will be `None` and the
/// row is filtered out.
#[derive(Debug, Deserialize)]
struct SearchNode {
    #[serde(rename = "__typename", default)]
    typename: Option<String>,
    #[serde(default)]
    number: Option<i64>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(rename = "isDraft", default)]
    is_draft: Option<bool>,
    #[serde(rename = "updatedAt", default)]
    updated_at: Option<String>,
    #[serde(rename = "headRefName", default)]
    head_ref_name: Option<String>,
    #[serde(default)]
    author: Option<NodeAuthor>,
    #[serde(default)]
    repository: Option<NodeRepository>,
    #[serde(rename = "headRepository", default)]
    head_repository: Option<NodeRepository>,
}

#[derive(Debug, Deserialize)]
struct NodeRepository {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

#[derive(Debug, Deserialize)]
struct NodeAuthor {
    #[serde(default)]
    login: Option<String>,
}

/// Parse the full `gh api graphql` search response into PR rows.
pub(crate) fn parse_search_response(stdout: &[u8]) -> Result<Vec<PullRequest>> {
    let response: SearchResponse = serde_json::from_slice(stdout).map_err(|err| {
        Error::Parse(format!(
            "{err} (input: {})",
            String::from_utf8_lossy(stdout)
        ))
    })?;
    response
        .data
        .search
        .nodes
        .into_iter()
        // Non-PR nodes (Issues sneaking through the search) are
        // tolerated rather than rejected so a stray match never
        // poisons the whole list.
        .filter(|node| {
            node.typename
                .as_deref()
                .map(|t| t == "PullRequest")
                .unwrap_or(true)
        })
        .map(into_pr)
        .collect()
}

fn into_pr(raw: SearchNode) -> Result<PullRequest> {
    let number = raw.number.ok_or_else(|| Error::Parse("missing `number`".to_owned()))?;
    let title = raw
        .title
        .ok_or_else(|| Error::Parse(format!("PR {number} missing `title`")))?;
    let url = raw
        .url
        .ok_or_else(|| Error::Parse(format!("PR {number} missing `url`")))?;
    let head_ref = raw
        .head_ref_name
        .ok_or_else(|| Error::Parse(format!("PR {number} missing `headRefName`")))?;
    let updated_at = raw
        .updated_at
        .ok_or_else(|| Error::Parse(format!("PR {number} missing `updatedAt`")))?;
    let base_repo = raw
        .repository
        .ok_or_else(|| Error::Parse(format!("PR {number} missing `repository`")))?;
    let (repo_owner, repo_name) = split_owner(&base_repo.name_with_owner)?;
    // A same-repo PR may have `headRepository: null` for very old
    // ancestry data; fall back to the base repo so cross-fork is the
    // only case where they actually differ.
    let (head_repo_owner, head_repo_name) = match raw.head_repository.as_ref() {
        Some(head) => split_owner(&head.name_with_owner)?,
        None => (repo_owner.clone(), repo_name.clone()),
    };
    // Ghost users have no login; surface a sentinel rather than
    // dropping the row entirely so the PR is still pickable.
    let author_login = raw
        .author
        .and_then(|author| author.login)
        .unwrap_or_else(|| "ghost".to_owned());

    Ok(PullRequest {
        number,
        title,
        repo_owner,
        repo_name,
        head_ref,
        head_repo_owner,
        head_repo_name,
        draft: raw.is_draft.unwrap_or(false),
        url,
        updated_at,
        author_login,
        // The local-clone join lives in the use case; the gateway has no
        // visibility into the registered-clone set, so the flag stays
        // `false` here and is rewritten upstream.
        has_local_clone: false,
    })
}

/// Split a `<owner>/<name>` repository reference. Reject anything that
/// does not have exactly one slash so a malformed response fails loudly
/// rather than producing nonsense rows.
fn split_owner(name_with_owner: &str) -> Result<(String, String)> {
    let mut parts = name_with_owner.splitn(2, '/');
    let owner = parts.next().filter(|s| !s.is_empty()).ok_or_else(|| {
        Error::Parse(format!(
            "repository name '{name_with_owner}' is missing an owner segment"
        ))
    })?;
    let name = parts.next().filter(|s| !s.is_empty()).ok_or_else(|| {
        Error::Parse(format!(
            "repository name '{name_with_owner}' is missing a name segment"
        ))
    })?;
    Ok((owner.to_owned(), name.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A captured `gh api graphql ...` fixture mirroring the shape the
    /// real call returns. Includes one same-repo PR and one cross-fork
    /// PR so both branches of `headRepository` are exercised.
    const FIXTURE: &str = r#"{
      "data": {
        "search": {
          "nodes": [
            {
              "__typename": "PullRequest",
              "number": 174,
              "title": "feat: add Repository tab to the new-session screen",
              "url": "https://github.com/x7c1/delta/pull/174",
              "isDraft": false,
              "updatedAt": "2026-06-20T11:33:21Z",
              "headRefName": "feat/repo-tab",
              "author": { "login": "x7c1" },
              "repository": { "nameWithOwner": "x7c1/delta" },
              "headRepository": { "nameWithOwner": "x7c1/delta" }
            },
            {
              "__typename": "PullRequest",
              "number": 173,
              "title": "wip: dropping worktree paths from Recent",
              "url": "https://github.com/forky/delta/pull/173",
              "isDraft": true,
              "updatedAt": "2026-06-19T08:00:00Z",
              "headRefName": "phaseA/recent",
              "author": { "login": "forky" },
              "repository": { "nameWithOwner": "x7c1/delta" },
              "headRepository": { "nameWithOwner": "forky/delta" }
            }
          ]
        }
      }
    }"#;

    #[test]
    fn parses_two_rows_with_the_expected_fields() {
        let rows = parse_search_response(FIXTURE.as_bytes()).unwrap();
        assert_eq!(rows.len(), 2);

        let pr = &rows[0];
        assert_eq!(pr.number, 174);
        assert_eq!(pr.title, "feat: add Repository tab to the new-session screen");
        assert_eq!(pr.repo_owner, "x7c1");
        assert_eq!(pr.repo_name, "delta");
        assert_eq!(pr.head_ref, "feat/repo-tab");
        assert_eq!(pr.head_repo_owner, "x7c1");
        assert_eq!(pr.head_repo_name, "delta");
        assert!(!pr.draft);
        assert_eq!(pr.author_login, "x7c1");
        assert!(!pr.has_local_clone, "the gateway never sets the clone flag");

        let cross = &rows[1];
        assert!(cross.draft);
        assert_eq!(cross.head_repo_owner, "forky", "cross-fork head is preserved");
        assert_eq!(cross.head_repo_name, "delta");
        assert_eq!(cross.repo_owner, "x7c1", "base repo owner stays the same");
    }

    #[test]
    fn missing_head_repository_falls_back_to_base() {
        let raw = r#"{
          "data": {
            "search": {
              "nodes": [{
                "__typename": "PullRequest",
                "number": 1, "title": "t", "url": "u",
                "isDraft": false,
                "updatedAt": "2026-01-01T00:00:00Z",
                "headRefName": "x",
                "author": { "login": "x7c1" },
                "repository": { "nameWithOwner": "x7c1/delta" }
              }]
            }
          }
        }"#;
        let rows = parse_search_response(raw.as_bytes()).unwrap();
        assert_eq!(rows[0].head_repo_owner, "x7c1");
        assert_eq!(rows[0].head_repo_name, "delta");
    }

    #[test]
    fn ghost_author_falls_back_to_sentinel_login() {
        let raw = r#"{
          "data": {
            "search": {
              "nodes": [{
                "__typename": "PullRequest",
                "number": 1, "title": "t", "url": "u",
                "isDraft": false,
                "updatedAt": "2026-01-01T00:00:00Z",
                "headRefName": "x",
                "repository": { "nameWithOwner": "x7c1/delta" }
              }]
            }
          }
        }"#;
        let rows = parse_search_response(raw.as_bytes()).unwrap();
        assert_eq!(rows[0].author_login, "ghost");
    }

    #[test]
    fn malformed_repository_name_is_an_error() {
        let raw = r#"{
          "data": {
            "search": {
              "nodes": [{
                "__typename": "PullRequest",
                "number": 1, "title": "t", "url": "u",
                "isDraft": false,
                "updatedAt": "2026-01-01T00:00:00Z",
                "headRefName": "x",
                "repository": { "nameWithOwner": "broken" }
              }]
            }
          }
        }"#;
        let err = parse_search_response(raw.as_bytes()).unwrap_err();
        assert!(
            matches!(err, Error::Parse(_)),
            "malformed owner is a Parse error, got {err:?}"
        );
    }

    #[test]
    fn empty_search_is_an_empty_vec() {
        let raw = r#"{ "data": { "search": { "nodes": [] } } }"#;
        let rows = parse_search_response(raw.as_bytes()).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn non_pr_nodes_are_filtered_rather_than_rejected() {
        // The GitHub search API returns ISSUE nodes too; a stray
        // Issue must not crash the parse.
        let raw = r#"{
          "data": {
            "search": {
              "nodes": [
                { "__typename": "Issue", "number": 9 },
                {
                  "__typename": "PullRequest",
                  "number": 1, "title": "t", "url": "u",
                  "isDraft": false,
                  "updatedAt": "2026-01-01T00:00:00Z",
                  "headRefName": "x",
                  "author": { "login": "x7c1" },
                  "repository": { "nameWithOwner": "x7c1/delta" }
                }
              ]
            }
          }
        }"#;
        let rows = parse_search_response(raw.as_bytes()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].number, 1);
    }
}
