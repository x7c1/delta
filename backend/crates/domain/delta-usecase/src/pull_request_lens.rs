//! Which PR search query backs a PR list.

use std::fmt;

/// Which PR search query backs a PR list.
///
/// `Reviewer` is "open PRs that requested my review and are not drafts" —
/// the inbox lens; `Author` is "open PRs I authored" — useful for picking
/// up your own in-flight work, which includes your own drafts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PullRequestLens {
    /// Open PRs that requested the authenticated user's review and are not
    /// drafts. Backs the "Requested for your review" section.
    Reviewer,
    /// Open PRs authored by the authenticated user, drafts included. Backs
    /// the "Yours" section so a still-draft branch is one click away.
    Author,
}

impl PullRequestLens {
    /// Parse the `lens=` query-string parameter the HTTP endpoint accepts.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "reviewer" => Some(Self::Reviewer),
            "author" => Some(Self::Author),
            _ => None,
        }
    }

    /// The canonical wire form used in the `lens=` query string and the
    /// per-lens cache key.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reviewer => "reviewer",
            Self::Author => "author",
        }
    }
}

impl fmt::Display for PullRequestLens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_the_two_lenses_and_rejects_others() {
        assert_eq!(
            PullRequestLens::parse("reviewer"),
            Some(PullRequestLens::Reviewer),
        );
        assert_eq!(
            PullRequestLens::parse("author"),
            Some(PullRequestLens::Author)
        );
        assert!(PullRequestLens::parse("everyone").is_none());
        assert!(PullRequestLens::parse("").is_none());
    }

    #[test]
    fn as_str_round_trips_through_parse() {
        for lens in [PullRequestLens::Reviewer, PullRequestLens::Author] {
            assert_eq!(PullRequestLens::parse(lens.as_str()), Some(lens));
        }
    }
}
