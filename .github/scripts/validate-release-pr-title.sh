#!/usr/bin/env bash
set -euo pipefail

# Validate that a release-PR title encodes a safe semver progression.
#
# Usage:
#   validate-release-pr-title.sh <pr_title> <last_tag>
#
# Arguments:
#   pr_title  — the pull-request title (e.g. "Release v0.2.0")
#   last_tag  — the previous release tag (e.g. "v0.1.0"); may be empty when
#               no release has been cut yet, in which case "v0.0.0" is used
#               as the implicit baseline.
#
# Rules:
#   - The title must match exactly `^Release v[0-9]+\.[0-9]+\.[0-9]+$`.
#   - The version it carries must be exactly one of:
#       * patch+1                       (0.3.10 -> 0.3.11)
#       * minor+1 with patch reset to 0 (0.3.10 -> 0.4.0)
#       * major+1 with minor and patch reset to 0 (0.3.10 -> 1.0.0)
#   - Any other change (downgrade, skip, malformed) fails with `::error::`.
#
# Exit codes:
#   0 — title is valid
#   1 — title is invalid (a `::error::` line is printed to stdout)
#   2 — usage error

TITLE_PATTERN='^Release v([0-9]+)\.([0-9]+)\.([0-9]+)$'
TAG_PATTERN='^v([0-9]+)\.([0-9]+)\.([0-9]+)$'

error() {
    printf '::error::%s\n' "$1"
}

usage() {
    cat >&2 <<'EOF'
Usage: validate-release-pr-title.sh <pr_title> <last_tag>
EOF
    exit 2
}

main() {
    if [ "$#" -ne 2 ]; then
        usage
    fi

    local title="$1"
    local last_tag="$2"

    if [[ ! "$title" =~ $TITLE_PATTERN ]]; then
        error "PR title '${title}' does not match the required pattern 'Release vX.Y.Z'."
        return 1
    fi

    local new_major="${BASH_REMATCH[1]}"
    local new_minor="${BASH_REMATCH[2]}"
    local new_patch="${BASH_REMATCH[3]}"

    local base_tag="${last_tag:-v0.0.0}"
    if [[ ! "$base_tag" =~ $TAG_PATTERN ]]; then
        error "Previous tag '${base_tag}' does not match the required pattern 'vX.Y.Z'."
        return 1
    fi

    local cur_major="${BASH_REMATCH[1]}"
    local cur_minor="${BASH_REMATCH[2]}"
    local cur_patch="${BASH_REMATCH[3]}"

    # patch bump: same major, same minor, patch increments by 1.
    if [ "$new_major" -eq "$cur_major" ] \
        && [ "$new_minor" -eq "$cur_minor" ] \
        && [ "$new_patch" -eq "$((cur_patch + 1))" ]; then
        echo "OK: patch bump ${base_tag} -> v${new_major}.${new_minor}.${new_patch}"
        return 0
    fi

    # minor bump: same major, minor increments by 1, patch resets to 0.
    if [ "$new_major" -eq "$cur_major" ] \
        && [ "$new_minor" -eq "$((cur_minor + 1))" ] \
        && [ "$new_patch" -eq 0 ]; then
        echo "OK: minor bump ${base_tag} -> v${new_major}.${new_minor}.${new_patch}"
        return 0
    fi

    # major bump: major increments by 1, minor and patch both reset to 0.
    if [ "$new_major" -eq "$((cur_major + 1))" ] \
        && [ "$new_minor" -eq 0 ] \
        && [ "$new_patch" -eq 0 ]; then
        echo "OK: major bump ${base_tag} -> v${new_major}.${new_minor}.${new_patch}"
        return 0
    fi

    error "Disallowed version transition ${base_tag} -> v${new_major}.${new_minor}.${new_patch}. Only patch+1, minor+1 (with patch=0), or major+1 (with minor=0 and patch=0) are accepted."
    return 1
}

main "$@"
