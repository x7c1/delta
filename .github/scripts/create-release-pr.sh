#!/usr/bin/env bash
set -euo pipefail

# Create or update the release PR on every push to main.
#
# The release PR is the single moving handle a developer touches to cut a
# new release: its title encodes the next version (e.g. "Release v0.2.0"),
# and merging it triggers the tagging workflow. This script rebuilds the
# release branch on every push to main so the PR always reflects the
# latest state, while preserving the developer-chosen title across runs.
#
# Behaviour:
#   - First run after a release: there is no open release PR. The default
#     next version is last_tag patch+1 (e.g. v0.1.0 -> v0.1.1), and a new
#     `release/v<X.Y.Z>` branch + PR are created.
#   - Subsequent runs while a release PR is open: the version is extracted
#     from the PR title (so the developer can promote it to a minor or
#     major bump by editing the title). The release branch is force-pushed
#     to a fresh commit on top of the latest main that bumps the version.
#
# In every code path, the chosen version is run through
# validate-release-pr-title.sh — the same logic the required check uses on
# pull_request events — so the bot can never produce a title that the
# required check would reject.
#
# Environment variables:
#   GH_TOKEN - GitHub token for API access (required).
#
# Outputs to GITHUB_OUTPUT (when set):
#   pr_number - the PR number (new or existing).
#   action    - "created" or "updated".

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=generate-changelog.sh
source "${SCRIPT_DIR}/generate-changelog.sh"

REPO_ROOT="$(git rev-parse --show-toplevel)"
CARGO_TOML="${REPO_ROOT}/backend/Cargo.toml"

main() {
    local pr_number last_tag current_version next_version repo_url changelog

    pr_number=$(find_existing_pr)
    last_tag=$(get_last_tag)
    current_version=$(read_workspace_version)
    repo_url=$(gh repo view --json url --jq '.url')

    if [ -n "$pr_number" ]; then
        next_version=$(version_from_pr_title "$pr_number")
        echo "Existing release PR #${pr_number} targets v${next_version}"
    else
        next_version=$(default_next_version "$last_tag")
        echo "No existing release PR; defaulting to v${next_version}"
    fi

    # Twice-guard the title: the dedicated workflow runs on pull_request
    # events, this script runs on push to main. Both must agree, so the
    # bot can never publish a title the required check would reject.
    "${SCRIPT_DIR}/validate-release-pr-title.sh" "Release v${next_version}" "$last_tag"

    changelog=$(generate_changelog "$last_tag" "$repo_url")

    if [ -n "$pr_number" ]; then
        update_release_pr "$pr_number" "$next_version" "$last_tag" "$changelog" "$repo_url"
        emit_output "pr_number" "$pr_number"
        emit_output "action" "updated"
    else
        ensure_release_label
        create_release_pr "$next_version" "$last_tag" "$changelog" "$repo_url"
        pr_number=$(find_existing_pr)
        emit_output "pr_number" "$pr_number"
        emit_output "action" "created"
    fi
}

find_existing_pr() {
    gh pr list --label "release" --state open --json number --jq '.[0].number // empty'
}

get_last_tag() {
    git describe --tags --abbrev=0 2>/dev/null || echo ""
}

# Read the workspace version through cargo metadata so the
# [workspace.package] section boundary is honoured by cargo itself.
read_workspace_version() {
    (cd "${REPO_ROOT}/backend" && \
        cargo metadata --no-deps --format-version 1 \
        | jq -r '.packages[] | select(.name == "delta-server") | .version')
}

# Extract X.Y.Z from a "Release vX.Y.Z" PR title. Bare semver (no leading
# "Release v") in the PR title would fail validation downstream anyway,
# but guard here so the failure surfaces close to the source.
version_from_pr_title() {
    local pr_number="$1"
    local title
    title=$(gh pr view "$pr_number" --json title --jq '.title')
    if [[ ! "$title" =~ ^Release\ v([0-9]+\.[0-9]+\.[0-9]+)$ ]]; then
        echo "::error::Release PR #${pr_number} title '${title}' does not match 'Release vX.Y.Z'." >&2
        exit 1
    fi
    echo "${BASH_REMATCH[1]}"
}

# Default next version when no release PR is open: patch+1 of the last
# tag. If there is no tag yet (very first release), start at 0.0.1.
default_next_version() {
    local last_tag="$1"
    if [ -z "$last_tag" ]; then
        echo "0.0.1"
        return
    fi
    if [[ ! "$last_tag" =~ ^v([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
        echo "::error::Last tag '${last_tag}' does not match 'vX.Y.Z'." >&2
        exit 1
    fi
    printf '%s.%s.%s\n' "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}" "$((BASH_REMATCH[3] + 1))"
}

ensure_release_label() {
    gh label create release --description "Release PR" --color 0E8A16 2>/dev/null || true
}

# Bump the workspace version via cargo set-version (cargo-edit). This
# expects `cargo install cargo-edit --locked` to have run earlier in the
# workflow.
bump_version_in_workspace() {
    local version="$1"
    (cd "${REPO_ROOT}/backend" && \
        cargo set-version --workspace "$version")
    # Keep Cargo.lock consistent with the new workspace version.
    (cd "${REPO_ROOT}/backend" && cargo update --workspace)
}

configure_bot_identity() {
    git config user.name "github-actions[bot]"
    git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
}

create_release_pr() {
    local version="$1"
    local last_tag="$2"
    local changelog="$3"
    local repo_url="$4"
    local branch="release/v${version}"

    configure_bot_identity

    # A previous failed run could have left the remote branch around.
    git push origin --delete "$branch" 2>/dev/null || true

    git checkout -b "$branch"

    bump_version_in_workspace "$version"

    git add "${CARGO_TOML}" "${REPO_ROOT}/backend/Cargo.lock"
    git commit -m "Release v${version}"
    git push origin "$branch"

    gh pr create \
        --title "Release v${version}" \
        --label "release" \
        --body "$(render_pr_body "$version" "$last_tag" "$changelog" "$repo_url")"
}

update_release_pr() {
    local pr_number="$1"
    local version="$2"
    local last_tag="$3"
    local changelog="$4"
    local repo_url="$5"
    local branch="release/v${version}"

    configure_bot_identity

    # The branch name encodes the version, so if the developer just
    # promoted the title (e.g. patch -> minor), the existing branch is
    # the wrong one. Recreate from the current main either way: that
    # also keeps the release commit on top of every new commit landed
    # since the last bot run.
    git fetch origin main
    git checkout -B "$branch" origin/main

    bump_version_in_workspace "$version"

    git add "${CARGO_TOML}" "${REPO_ROOT}/backend/Cargo.lock"
    git commit -m "Release v${version}"
    git push origin "$branch" --force-with-lease

    gh pr edit "$pr_number" \
        --title "Release v${version}" \
        --body "$(render_pr_body "$version" "$last_tag" "$changelog" "$repo_url")"
}

# PR body layout: changelog first, then a compare link against last_tag
# pointing at the release branch (rewritten to point at the tag once the
# release workflow has run — see update-release-pr-links.sh).
render_pr_body() {
    local version="$1"
    local last_tag="$2"
    local changelog="$3"
    local repo_url="$4"
    local compare=""

    if [ -n "$last_tag" ]; then
        compare="## Changelog

- [${last_tag}...v${version}](${repo_url}/compare/${last_tag}...release/v${version})"
    fi

    cat <<EOF
${changelog}
${compare}
EOF
}

emit_output() {
    local key="$1"
    local value="$2"
    if [ -n "${GITHUB_OUTPUT:-}" ]; then
        echo "${key}=${value}" >> "$GITHUB_OUTPUT"
    fi
    echo "${key}=${value}"
}

main "$@"
