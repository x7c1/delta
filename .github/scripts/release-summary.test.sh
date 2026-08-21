#!/usr/bin/env bash
set -uo pipefail

# Unit tests for release-summary.sh.
#
# Usage:
#   bash release-summary.test.sh
#
# The marker is spelled out literally here on purpose: asserting the split
# against a hardcoded string is what makes this an independent check rather
# than a tautology over changelog_marker's own output.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=release-summary.sh
source "${SCRIPT_DIR}/release-summary.sh"

MARKER='<!-- changelog:auto -->'
SENTINEL='<!-- release-summary:todo -->'

failures=0

pass() {
    printf 'ok   - %s\n' "$1"
}

fail() {
    printf 'FAIL - %s\n' "$1"
    printf '%s\n' "$2"
    failures=$((failures + 1))
}

assert_eq() {
    local name="$1" expected="$2" actual="$3"
    if [ "$expected" = "$actual" ]; then
        pass "$name"
    else
        fail "$name" "  expected: $(printf '%q' "$expected")
  actual:   $(printf '%q' "$actual")"
    fi
}

assert_not_contains() {
    local name="$1" haystack="$2" needle="$3"
    if [[ "$haystack" != *"$needle"* ]]; then
        pass "$name"
    else
        fail "$name" "  unexpectedly found $(printf '%q' "$needle") in $(printf '%q' "$haystack")"
    fi
}

assert_unwritten() {
    local name="$1" summary="$2"
    if summary_is_unwritten "$summary"; then
        pass "$name"
    else
        fail "$name" "  expected unwritten, got written: $(printf '%q' "$summary")"
    fi
}

assert_written() {
    local name="$1" summary="$2"
    if summary_is_unwritten "$summary"; then
        fail "$name" "  expected written, got unwritten: $(printf '%q' "$summary")"
    else
        pass "$name"
    fi
}

# changelog_marker is the one definition every other caller uses.
assert_eq "changelog_marker echoes the marker" "$MARKER" "$(changelog_marker)"

# --- summary_region ---------------------------------------------------------

body_with_marker=$(printf '%s\n' \
    '## Summary' \
    '' \
    'Sessions now survive a restart.' \
    '' \
    "$MARKER" \
    '' \
    '## Features' \
    '' \
    '- feat: persist sessions')

region=$(summary_region "$body_with_marker")

assert_eq "summary_region returns the human region" \
    '## Summary

Sessions now survive a restart.' \
    "$region"
assert_not_contains "summary_region drops the marker line" "$region" "$MARKER"
assert_not_contains "summary_region drops the changelog below" "$region" '## Features'

# Command substitution eats trailing newlines, so compare with a trailing
# sentinel character on both sides to see the blank lines that separated the
# summary from the marker.
region_raw="$(summary_region "$body_with_marker"; printf 'X')"
expected_raw="$(printf '%s\n' \
    '## Summary' \
    '' \
    'Sessions now survive a restart.'; printf 'X')"
assert_eq "summary_region trims the blank lines above the marker" \
    "$expected_raw" "$region_raw"

# A code fence in the human region may quote the marker; the split must
# still happen at the first occurrence, not the last.
body_with_fenced_marker=$(printf '%s\n' \
    '## Summary' \
    '' \
    'The generated body looks like this:' \
    '' \
    '```markdown' \
    "$MARKER" \
    '```' \
    '' \
    "$MARKER" \
    '' \
    '## Features' \
    '' \
    '- feat: persist sessions')

fenced_region=$(summary_region "$body_with_fenced_marker")

assert_eq "summary_region splits at the first marker occurrence" \
    '## Summary

The generated body looks like this:

```markdown' \
    "$fenced_region"
assert_not_contains "summary_region keeps the changelog out of a fenced body" \
    "$fenced_region" '## Features'

body_without_marker=$(printf '%s\n' \
    '## Features' \
    '' \
    '- feat: persist sessions')

assert_eq "summary_region returns empty for a marker-less body" \
    '' \
    "$(summary_region "$body_without_marker")"

# --- summary_is_unwritten ---------------------------------------------------

assert_unwritten "empty summary is unwritten" ''
assert_unwritten "whitespace-only summary is unwritten" "$(printf '  \n\t\n')"
assert_unwritten "sentinel-bearing summary is unwritten" "$(printf '%s\n' \
    '## Summary' \
    '' \
    "$SENTINEL" \
    '_Write the release summary here._')"
assert_unwritten "the placeholder is unwritten" "$(summary_placeholder)"
assert_unwritten "a comment-only summary is unwritten" \
    "$(printf '<!-- nothing\nto see here -->\n')"

assert_written "a written summary is written" "$(printf '%s\n' \
    '## Summary' \
    '' \
    'Sessions now survive a restart.')"

# The sentinel is matched as an exact comment, so prose is free to use the
# word TODO like any other word.
assert_written "prose containing the word TODO is written" "$(printf '%s\n' \
    '## Summary' \
    '' \
    'Adds a TODO list to the session pane.')"

# The placeholder round-trips through a generated body.
generated_body=$(printf '%s\n\n%s\n\n%s\n' \
    "$(summary_placeholder)" \
    "$MARKER" \
    '## Features')
assert_eq "summary_region recovers the placeholder from a generated body" \
    "$(summary_placeholder)" \
    "$(summary_region "$generated_body")"

if [ "$failures" -ne 0 ]; then
    printf '\n%d test(s) failed.\n' "$failures"
    exit 1
fi

printf '\nAll release-summary tests passed.\n'
