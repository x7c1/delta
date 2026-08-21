#!/usr/bin/env bash
# Shared helpers for the hand-written summary region of the release PR body.
#
# The release PR body is split in two by a marker line: everything above it
# is written by a human and is preserved across regeneration, everything
# below it is rebuilt from `git log` on every push to main. The summary
# region is what the Release workflow publishes as the GitHub Release body.
# The layout and the marker are documented in docs/guides/release.md; this
# script holds the only definition of the marker string, so no caller
# hardcodes it a second time.
#
# Usage:
#   source release-summary.sh
#   summary=$(summary_region "$body")
#
# Or standalone:
#   bash release-summary.sh marker
#   bash release-summary.sh placeholder
#   bash release-summary.sh region < body.md
#
# Every function here is pure text manipulation — no network, no GitHub CLI
# — so release-summary.test.sh can exercise them offline. The PR lookup that
# needs the GitHub CLI lives in release-pr-lookup.sh instead.

# The single definition of the marker that separates the human region from
# the machine-generated changelog.
changelog_marker() {
    printf '%s\n' '<!-- changelog:auto -->'
}

# The sentinel left in a freshly generated body, marking a summary that has
# not been written yet.
summary_sentinel() {
    printf '%s\n' '<!-- release-summary:todo -->'
}

# The starting point for a summary nobody has written yet.
summary_placeholder() {
    cat <<EOF
## Summary

$(summary_sentinel)
_Write the release summary here. It becomes the body of the GitHub Release.
Delete the marker comment above once written._
EOF
}

# Echo everything above the first occurrence of the marker, with trailing
# blank lines trimmed. Echo nothing when the marker is absent: such a body
# predates the marker and is wholly machine-generated, so it carries no
# summary to preserve.
#
# The split is on the exact marker string rather than a regular expression,
# so arbitrary Markdown in the human region (code fences, nested comments,
# horizontal rules) cannot corrupt it.
summary_region() {
    local body="$1"
    local marker region

    marker=$(changelog_marker)
    if [[ "$body" != *"$marker"* ]]; then
        return 0
    fi

    # `%%` strips the longest suffix starting at the marker, i.e. the split
    # happens at its first occurrence.
    region="${body%%"$marker"*}"
    region="${region%"${region##*[![:space:]]}"}"

    if [ -z "$region" ]; then
        return 0
    fi
    printf '%s\n' "$region"
}

# True when the summary cannot serve as a release body: empty, whitespace
# only once HTML comments are removed, or still carrying the sentinel.
summary_is_unwritten() {
    local summary="$1"
    local sentinel stripped

    sentinel=$(summary_sentinel)
    if [[ "$summary" == *"$sentinel"* ]]; then
        return 0
    fi

    stripped=$(strip_html_comments "$summary")
    [ -z "${stripped//[[:space:]]/}" ]
}

# Remove every complete `<!-- ... -->` comment, including multi-line ones.
strip_html_comments() {
    local text="$1"
    local head tail

    while [[ "$text" == *'<!--'*'-->'* ]]; do
        head="${text%%'<!--'*}"
        tail="${text#*'<!--'}"
        tail="${tail#*'-->'}"
        text="${head}${tail}"
    done
    printf '%s' "$text"
}

# Allow standalone execution.
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    set -euo pipefail
    case "${1:-}" in
        marker)
            changelog_marker
            ;;
        placeholder)
            summary_placeholder
            ;;
        region)
            summary_region "$(cat)"
            ;;
        *)
            echo "Usage: release-summary.sh {marker|placeholder|region}" >&2
            exit 2
            ;;
    esac
fi
