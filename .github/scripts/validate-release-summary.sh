#!/usr/bin/env bash
set -euo pipefail

# Fail when a release PR body carries no hand-written summary.
#
# Usage:
#   validate-release-summary.sh <pr_body>
#   validate-release-summary.sh < pr_body
#
# The summary region is what the Release workflow publishes as the GitHub
# Release body, so a release must not be merged without it. The body layout
# is documented in docs/guides/release.md.
#
# Exit codes:
#   0 — a summary is present
#   1 — the summary is missing or still unwritten (a `::error::` line is printed)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=release-summary.sh
source "${SCRIPT_DIR}/release-summary.sh"

main() {
    local body summary

    if [ "$#" -ge 1 ]; then
        body="$1"
    else
        body="$(cat)"
    fi

    summary=$(summary_region "$body")

    if summary_is_unwritten "$summary"; then
        printf '::error::%s\n' "The release PR has no release summary. Write it above the '$(changelog_marker)' marker in the PR body and delete the '$(summary_sentinel)' comment: that text is published as the GitHub Release body. See docs/guides/release.md."
        return 1
    fi

    echo "OK: the release PR carries a summary."
}

main "$@"
