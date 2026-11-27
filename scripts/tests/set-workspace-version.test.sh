#!/usr/bin/env bash
#
# Tests for scripts/set-workspace-version.sh — the version writer of the
# release PR's commit.
#
# Usage: bash scripts/tests/set-workspace-version.test.sh
#
# Each case writes a throwaway cargo workspace under `mktemp -d`, so the
# repository itself is never touched: a `delta-server` member and a `helper`
# member that depends on it by path, both taking their version from
# [workspace.package], with a Cargo.lock generated for them. A table after
# [workspace.package] holds a `version` key of its own, which the script must
# leave alone. The workspace has no registry dependencies, so every cargo
# command runs offline: what is under test is the workspace version and the
# lock entries of the workspace's own crates, the only entries the script
# changes. The workspace lies outside this repository, so cargo uses the
# default toolchain rather than the one rust-toolchain.toml pins. Needs bash,
# cargo and jq.
#
# The script exits non-zero on the first failed assertion.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SET="$(cd "$SCRIPT_DIR/.." && pwd)/set-workspace-version.sh"
readonly MEMBERS=(delta-server helper)

[ -x "$SET" ] || {
  printf 'FAIL - set-workspace-version script is not executable: %s\n' "$SET" >&2
  exit 1
}
for tool in cargo jq; do
  command -v "$tool" >/dev/null 2>&1 || {
    printf 'FAIL - %s not found on PATH; the script under test needs it\n' "$tool" >&2
    exit 1
  }
done

# Nothing here needs the network; say so, so that cargo never reaches for it.
export CARGO_NET_OFFLINE=true

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/delta-set-workspace-version-test.XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT

pass() { printf 'ok   - %s\n' "$1"; }

fail() {
  printf 'FAIL - %s\n' "$1" >&2
  [ $# -gt 1 ] && printf '%s\n' "$2" >&2
  exit 1
}

# Writes a workspace into directory $1 at version $2, with a Cargo.lock. With
# $3 set to `no-version`, [workspace.package] holds no version line, and the
# members name their versions themselves.
make_workspace() {
  local dir="$1" version="$2" variant="${3:-}" crate member_version
  member_version='version.workspace = true'
  [ "$variant" != no-version ] || member_version="version = \"$version\""
  mkdir -p "$dir"
  for crate in "${MEMBERS[@]}"; do
    mkdir -p "$dir/crates/$crate/src"
    : >"$dir/crates/$crate/src/lib.rs"
    printf '[package]\nname = "%s"\n%s\nedition.workspace = true\n' "$crate" "$member_version" \
      >"$dir/crates/$crate/Cargo.toml"
  done
  printf '\n[dependencies]\ndelta-server = { path = "../delta-server" }\n' \
    >>"$dir/crates/helper/Cargo.toml"
  {
    printf '[workspace]\nresolver = "2"\nmembers = ["crates/*"]\n\n'
    printf '[workspace.package]\n'
    [ "$variant" = no-version ] || printf 'version = "%s"  # the release version\n' "$version"
    printf 'edition = "2021"\n\n'
    printf '[workspace.metadata.unrelated]\nversion = "untouched"\n'
  } >"$dir/Cargo.toml"
  (cd "$dir" && cargo generate-lockfile --quiet) || fail "cargo generate-lockfile in $dir"
}

# The version Cargo.lock in $1 records for crate $2.
locked_version() {
  awk -v crate="$2" '
    $0 == "name = \"" crate "\"" { found = 1; next }
    found && /^version = / { gsub(/^version = "|"$/, ""); print; exit }
  ' "$1/Cargo.lock"
}

# The version `cargo metadata` reports for crate $2 of the workspace in $1.
metadata_version() {
  (cd "$1" && cargo metadata --no-deps --format-version 1 |
    jq -r --arg crate "$2" '.packages[] | select(.name == $crate) | .version')
}

# Sets the workspace of case $1 from 0.1.0 to version $2 and checks
# everything that should then read $2.
expect_set() {
  local name="$1" version="$2" dir="$WORK_DIR/$1" log="$WORK_DIR/$1.log" crate actual
  make_workspace "$dir" 0.1.0
  "$SET" "$version" "$dir" >"$log" 2>&1 || fail "$name: the script failed" "$(cat "$log")"
  grep -qx "version = \"$version\"  # the release version" "$dir/Cargo.toml" ||
    fail "$name: Cargo.toml does not hold version = \"$version\"" "$(cat "$dir/Cargo.toml")"
  grep -qx 'version = "untouched"' "$dir/Cargo.toml" ||
    fail "$name: the version of another table was changed" "$(cat "$dir/Cargo.toml")"
  for crate in "${MEMBERS[@]}"; do
    actual=$(metadata_version "$dir" "$crate")
    [ "$actual" = "$version" ] || fail "$name: cargo metadata reads $crate at '$actual'"
    actual=$(locked_version "$dir" "$crate")
    [ "$actual" = "$version" ] ||
      fail "$name: Cargo.lock holds $crate at '$actual'" "$(cat "$dir/Cargo.lock")"
  done
  (cd "$dir" && cargo metadata --locked --offline --format-version 1 >/dev/null 2>"$log") ||
    fail "$name: cargo metadata --locked --offline refused the result" "$(cat "$log")"
  pass "$name: sets the workspace, delta-server and Cargo.lock to $version"
}

# Runs the script with the arguments after $3 against case $1, prepared as
# workspace variant $2, expecting it to fail, to say $3, and to leave
# Cargo.toml and Cargo.lock as they were.
expect_refusal() {
  local name="$1" variant="$2" said="$3" dir="$WORK_DIR/$1" log="$WORK_DIR/$1.log"
  shift 3
  make_workspace "$dir" 0.1.0 "$variant"
  cp "$dir/Cargo.toml" "$WORK_DIR/$name.toml"
  cp "$dir/Cargo.lock" "$WORK_DIR/$name.lock"
  if "$SET" "$@" "$dir" >"$log" 2>&1; then
    fail "$name: the script succeeded" "$(cat "$log")"
  fi
  grep -qF -- "$said" "$log" || fail "$name: the script failed without saying: $said" "$(cat "$log")"
  cmp -s "$dir/Cargo.toml" "$WORK_DIR/$name.toml" && cmp -s "$dir/Cargo.lock" "$WORK_DIR/$name.lock" ||
    fail "$name: Cargo.toml or Cargo.lock was changed"
  pass "$name: refused, leaving the workspace as it was"
}

expect_set patch 0.1.1
expect_set minor 0.2.0
expect_set major 1.0.0

expect_refusal tag-not-version '' 'is not a version of the form X.Y.Z' v0.2.0
expect_refusal pre-release '' 'is not a version of the form X.Y.Z' 0.2.0-rc.1
expect_refusal no-version-line no-version 'does not have exactly one version line' 0.2.0

printf 'all set-workspace-version tests passed\n'
