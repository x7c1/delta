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
#   - First run after a release: there is no open release PR. A fresh
#     `release/since-<UTC date and time>` branch is opened with the
#     default next version (patch+1 of last_tag, e.g. v0.1.0 -> v0.1.1).
#   - Subsequent runs while a release PR is open: reuse the PR's head
#     branch (queried via `gh pr view --json headRefName`) so the branch
#     pointer the PR follows stays the same across force-pushes. This is
#     what lets the developer promote the title (patch -> minor/major)
#     without orphaning the PR — the version lives in the title and the
#     Cargo.toml, while the branch name is intentionally decoupled and
#     fixed for the lifetime of the PR.
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
    local pr_number last_tag current_version next_version repo_url changelog branch

    pr_number=$(find_existing_pr)
    last_tag=$(get_last_tag)
    current_version=$(read_workspace_version)
    repo_url=$(gh repo view --json url --jq '.url')

    if [ -n "$pr_number" ]; then
        next_version=$(version_from_pr_title "$pr_number")
        branch=$(branch_from_pr "$pr_number")
        echo "Existing release PR #${pr_number} on ${branch} targets v${next_version}"
    else
        next_version=$(default_next_version "$last_tag")
        branch=$(new_release_branch)
        echo "No existing release PR; opening ${branch} for v${next_version}"
    fi

    # Twice-guard the title: the dedicated workflow runs on pull_request
    # events, this script runs on push to main. Both must agree, so the
    # bot can never publish a title the required check would reject.
    "${SCRIPT_DIR}/validate-release-pr-title.sh" "Release v${next_version}" "$last_tag"

    changelog=$(generate_changelog "$last_tag" "$repo_url")

    if [ -n "$pr_number" ]; then
        update_release_pr "$pr_number" "$branch" "$next_version" "$last_tag" "$changelog" "$repo_url"
        emit_output "pr_number" "$pr_number"
        emit_output "action" "updated"
    else
        ensure_release_label
        create_release_pr "$branch" "$next_version" "$last_tag" "$changelog" "$repo_url"
        pr_number=$(find_existing_pr)
        emit_output "pr_number" "$pr_number"
        emit_output "action" "created"
    fi
}

find_existing_pr() {
    gh pr list --label "release" --state open --json number --jq '.[0].number // empty'
}

# Read the head branch from an existing release PR. We reuse that
# branch on every force-push so the PR keeps pointing at our work
# regardless of how the title has been promoted.
branch_from_pr() {
    local pr_number="$1"
    gh pr view "$pr_number" --json headRefName --jq '.headRefName'
}

# Mint a fresh release-PR branch. Use UTC so the name is independent
# of the runner's timezone and reproducible from the workflow log.
new_release_branch() {
    printf 'release/since-%s\n' "$(date -u +'%Y-%m-%d-%H%M')"
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
    local branch="$1"
    local version="$2"
    local last_tag="$3"
    local changelog="$4"
    local repo_url="$5"

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
        --head "$branch" \
        --body "$(render_pr_body "$branch" "$version" "$last_tag" "$changelog" "$repo_url")"
}

update_release_pr() {
    local pr_number="$1"
    local branch="$2"
    local version="$3"
    local last_tag="$4"
    local changelog="$5"
    local repo_url="$6"

    configure_bot_identity

    # Rebuild the PR's existing head branch from the current main and
    # re-apply the version bump on top. Reusing the same branch name is
    # what keeps the PR's head pointer in sync with our push, even when
    # the developer has promoted the title (patch -> minor/major).
    git fetch origin main
    git checkout -B "$branch" origin/main

    bump_version_in_workspace "$version"

    git add "${CARGO_TOML}" "${REPO_ROOT}/backend/Cargo.lock"
    git commit -m "Release v${version}"
    git push origin "$branch" --force-with-lease

    gh pr edit "$pr_number" \
        --title "Release v${version}" \
        --body "$(render_pr_body "$branch" "$version" "$last_tag" "$changelog" "$repo_url")"
}

# PR body layout: changelog first, then a compare link against last_tag
# pointing at the release branch (rewritten to point at the tag once the
# release workflow has run — see update-release-pr-links.sh).
render_pr_body() {
    local branch="$1"
    local version="$2"
    local last_tag="$3"
    local changelog="$4"
    local repo_url="$5"
    local compare=""

    if [ -n "$last_tag" ]; then
        compare="## Changelog

- [${last_tag}...v${version}](${repo_url}/compare/${last_tag}...${branch})"
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
