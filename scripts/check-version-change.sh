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
#   - The previous version is obtained by temporarily checking out
#     backend/Cargo.toml from HEAD~1 and running cargo metadata against
#     that snapshot, then restoring the working-tree copy. cargo metadata
#     only reads Cargo.toml + Cargo.lock; no network access is required.

REPO_ROOT="$(git rev-parse --show-toplevel)"
CARGO_TOML="${REPO_ROOT}/backend/Cargo.toml"

read_workspace_version() {
    # delta-server pulls its version from [workspace.package].version, so
    # reading any workspace member would do — picking one explicitly keeps
    # the intent obvious.
    (cd "${REPO_ROOT}/backend" && \
        cargo metadata --no-deps --format-version 1 \
        | jq -r '.packages[] | select(.name == "delta-server") | .version')
}

current_version=$(read_workspace_version)

# Stash the working-tree Cargo.toml so we can restore it after reading
# the HEAD~1 snapshot.
backup=$(mktemp)
cp "$CARGO_TOML" "$backup"
trap 'cp "$backup" "$CARGO_TOML"; rm -f "$backup"' EXIT

git show HEAD~1:backend/Cargo.toml > "$CARGO_TOML"
previous_version=$(read_workspace_version)

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
