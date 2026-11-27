#!/usr/bin/env bash
set -euo pipefail

# Set the workspace version: [workspace.package].version of the cargo
# workspace in the given directory (backend/ of this repository by default),
# and bring that workspace's Cargo.lock in line with it.
#
# Usage:
#   set-workspace-version.sh <version> [<workspace directory>]
#
# The Release PR workflow (.github/workflows/release-pr.yml) runs this as
# colophon's `command` version writer, with the version the release PR's title
# names; scripts/tests/set-workspace-version.test.sh runs it against throwaway
# workspaces. The version is X.Y.Z, which is what colophon's semver scheme
# produces. See docs/guides/release.md ("Setting the version").
#
# It runs where colophon's job runs it: a fresh ubuntu-latest runner with this
# repository checked out at its root and nothing installed beyond the runner
# image, with no step before it to install anything. So:
#
#   - The toolchain. rustup does not install the toolchain rust-toolchain.toml
#     names on first use, so `rustup toolchain install` (rustup 1.28 or later)
#     runs in the workspace directory first. With no toolchain named it
#     installs the one active there (the pinned one in this repository, the
#     default one in a throwaway workspace outside it) and does nothing when
#     it is already installed. `--profile minimal` leaves out the
#     documentation, which nothing here reads and which is most of the
#     download.
#   - Cargo.toml. The quoted value of the one `version = "..."` line of the
#     [workspace.package] table is rewritten in place with awk, rather than
#     through cargo-edit's `cargo set-version`, which the runner does not carry
#     and which would take minutes to build. The line is looked for within that
#     table only, and the script fails unless exactly one was rewritten. It
#     then reads delta-server's version back through `cargo metadata` and fails
#     unless cargo sees the new one, so a manifest the rewrite did not
#     understand stops here rather than in the release PR's commit.
#   - Cargo.lock. `cargo update --workspace` rewrites the entries of the
#     workspace's own crates and leaves every other locked dependency where it
#     is. Cargo writing the lock file itself, rather than a text edit of it,
#     keeps the file exactly as cargo would have it, so a later `--locked`
#     build accepts it. It builds nothing.
#
# Nothing is written outside the workspace's Cargo.toml and Cargo.lock, since
# colophon commits whatever the command leaves in the working tree. Needs
# rustup (or cargo) and jq, both on the runner image.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT

if [ $# -lt 1 ] || [ $# -gt 2 ]; then
    echo "usage: set-workspace-version.sh <version> [<workspace directory>]" >&2
    exit 2
fi
version="$1"
workspace="${2:-$REPO_ROOT/backend}"

if [[ ! "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
    echo "set-workspace-version: '${version}' is not a version of the form X.Y.Z" >&2
    exit 1
fi

manifest="${workspace}/Cargo.toml"
if [ ! -f "$manifest" ]; then
    echo "set-workspace-version: there is no Cargo.toml in ${workspace}" >&2
    exit 1
fi

if command -v rustup >/dev/null 2>&1; then
    (cd "$workspace" && rustup toolchain install --profile minimal)
fi

# The manifest with the version line of [workspace.package] replaced; awk
# exits 3 unless exactly one such line was found. A table header is a line
# starting with `[`; an array of tables (`[[...]]`) ends the table as well.
rewritten="$(mktemp)"
trap 'rm -f "$rewritten"' EXIT
status=0
awk -v version="$version" '
    /^[[:space:]]*\[/ {
        header = $0
        sub(/[[:space:]]*#.*$/, "", header)
        gsub(/[[:space:]]/, "", header)
        in_table = (header == "[workspace.package]")
    }
    in_table && /^[[:space:]]*version[[:space:]]*=/ {
        sub(/"[^"]*"/, "\"" version "\"")
        replaced++
    }
    { print }
    END { if (replaced != 1) exit 3 }
' "$manifest" >"$rewritten" || status=$?
if [ "$status" -ne 0 ]; then
    if [ "$status" -eq 3 ]; then
        echo "set-workspace-version: ${manifest} does not have exactly one version line in [workspace.package]" >&2
    fi
    exit 1
fi
cat "$rewritten" >"$manifest"

(cd "$workspace" && cargo update --workspace)

# delta-server takes its version from [workspace.package], as every member
# does; it is the crate the release is named after.
actual=$(cd "$workspace" &&
    cargo metadata --no-deps --format-version 1 |
    jq -r '.packages[] | select(.name == "delta-server") | .version')
if [ "$actual" != "$version" ]; then
    echo "set-workspace-version: cargo reads delta-server's version as '${actual}' after setting the workspace to ${version}" >&2
    exit 1
fi
echo "The workspace in ${workspace} is at version ${version}."
