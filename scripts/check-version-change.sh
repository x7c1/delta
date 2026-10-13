#!/usr/bin/env bash
set -euo pipefail

# Detect whether the workspace version in backend/Cargo.toml changed between
# HEAD~1 and HEAD, and report the result via $GITHUB_OUTPUT so a workflow
# step can branch on it.
#
# Outputs (to $GITHUB_OUTPUT, when set):
#   changed=true|false
#   version=<semver>      — only when changed=true; the new version.
#
# Implementation notes:
#   - The version is read through `cargo metadata` rather than by grepping
#     TOML, so the `[workspace.package]` section boundary is honoured by
#     cargo itself.
#   - The current version is read from the working tree's backend/.
#   - The previous version is read from a copy of HEAD~1's backend/ alone,
#     extracted with `git archive` into a temporary directory. Reading
#     HEAD~1's root manifest against HEAD's member manifests would mix two
#     trees: when HEAD adds a [workspace.dependencies] entry that a member
#     inherits (`foo.workspace = true`), the old root lacks it and cargo
#     cannot load the member. `cargo metadata --no-deps` reads only the
#     manifests (and the target sources they imply); no Cargo.lock and no
#     network access are required.

REPO_ROOT="$(git rev-parse --show-toplevel)"

read_workspace_version() {
    # delta-server pulls its version from [workspace.package].version, so
    # reading any workspace member would do — picking one explicitly keeps
    # the intent obvious.
    local backend_dir="$1"
    local version
    version=$(cd "$backend_dir" && \
        cargo metadata --no-deps --format-version 1 \
        | jq -r '.packages[] | select(.name == "delta-server") | .version') || return 1
    if [ -z "$version" ]; then
        echo "check-version-change: no delta-server package in ${backend_dir}" >&2
        return 1
    fi
    printf '%s\n' "$version"
}

current_version=$(read_workspace_version "${REPO_ROOT}/backend")

previous_tree=$(mktemp -d "${TMPDIR:-/tmp}/delta-version-check.XXXXXX")
trap 'rm -rf "$previous_tree"' EXIT

git -C "$REPO_ROOT" archive --format=tar HEAD~1 backend | tar -x -C "$previous_tree"
previous_version=$(read_workspace_version "${previous_tree}/backend")

echo "Current version: ${current_version}"
echo "Previous version: ${previous_version}"

if [ -z "${GITHUB_OUTPUT:-}" ]; then
    GITHUB_OUTPUT=/dev/null
fi

if [ "$current_version" != "$previous_version" ]; then
    echo "Version changed from ${previous_version} to ${current_version}"
    {
        echo "changed=true"
        echo "version=${current_version}"
    } >> "$GITHUB_OUTPUT"
else
    echo "Version unchanged; skipping release"
    echo "changed=false" >> "$GITHUB_OUTPUT"
fi
