#!/usr/bin/env bash
#
# Tests for scripts/check-version-change.sh — the Release workflow's decision
# whether HEAD bumped the workspace version.
#
# Usage: bash scripts/tests/check-version-change.test.sh
#
# Each case builds a scratch git repository under `mktemp -d` holding a
# minimal `backend/` cargo workspace (a `delta-server` member that inherits
# the workspace version, and a `helper` member), commits a base, commits one
# change on top, and runs the script with the scratch repository as the
# working directory — the script finds its repository through
# `git rev-parse --show-toplevel`. The verdict is read from the file
# $GITHUB_OUTPUT points at. Needs bash, git, cargo and jq; `cargo metadata
# --no-deps` resolves nothing, so no network access is required.
#
# The script exits non-zero on the first failed assertion.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECK="$(cd "$SCRIPT_DIR/.." && pwd)/check-version-change.sh"

[ -x "$CHECK" ] || {
  printf 'FAIL - check-version-change script is not executable: %s\n' "$CHECK" >&2
  exit 1
}
for tool in git cargo jq; do
  command -v "$tool" >/dev/null 2>&1 || {
    printf 'FAIL - %s not found on PATH; the script under test needs it\n' "$tool" >&2
    exit 1
  }
done

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/delta-version-check-test.XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT

pass() { printf 'ok   - %s\n' "$1"; }

fail() {
  printf 'FAIL - %s\n' "$1" >&2
  [ $# -gt 1 ] && printf '%s\n' "$2" >&2
  exit 1
}

assert_eq() {
  if [ "$2" = "$3" ]; then
    pass "$1"
  else
    fail "$1" "  expected: $(printf '%q' "$2")
  actual:   $(printf '%q' "$3")"
  fi
}

# Commits in the scratch repository without depending on the host's git
# identity, signing setup or hooks.
scratch_git() {
  git -C "$REPO" \
    -c user.name=delta-test -c user.email=delta-test@example.invalid \
    -c commit.gpgsign=false -c core.hooksPath=/dev/null \
    "$@"
}

# Writes the workspace root manifest. $1 is the workspace version; $2, when
# non-empty, adds `helper` to [workspace.dependencies].
write_root_manifest() {
  {
    printf '[workspace]\nresolver = "2"\nmembers = ["crates/*"]\n\n'
    printf '[workspace.package]\nversion = "%s"\nedition = "2021"\n' "$1"
    if [ -n "$2" ]; then
      printf '\n[workspace.dependencies]\nhelper = { path = "crates/helper" }\n'
    fi
  } >"$REPO/backend/Cargo.toml"
}

# Writes delta-server's manifest; $1, when non-empty, makes it inherit the
# `helper` workspace dependency.
write_server_manifest() {
  {
    printf '[package]\nname = "delta-server"\nversion.workspace = true\nedition.workspace = true\n'
    if [ -n "$1" ]; then
      printf '\n[dependencies]\nhelper.workspace = true\n'
    fi
  } >"$REPO/backend/crates/delta-server/Cargo.toml"
}

# A fresh scratch repository at version 0.1.0 with no workspace dependencies,
# committed as the base.
new_repo() {
  REPO="$WORK_DIR/$1"
  mkdir -p "$REPO/backend/crates/delta-server/src" "$REPO/backend/crates/helper/src"
  git init -q "$REPO"
  write_root_manifest 0.1.0 ""
  write_server_manifest ""
  printf 'fn main() {}\n' >"$REPO/backend/crates/delta-server/src/main.rs"
  printf '[package]\nname = "helper"\nversion.workspace = true\nedition.workspace = true\n' \
    >"$REPO/backend/crates/helper/Cargo.toml"
  : >"$REPO/backend/crates/helper/src/lib.rs"
  scratch_git add -A
  scratch_git commit -q -m base
}

commit_change() {
  scratch_git add -A
  scratch_git commit -q -m change
}

RUN_STATUS=0
RUN_OUT=""
OUTPUT_FILE=""
run_check() {
  OUTPUT_FILE="$WORK_DIR/$1.github-output"
  : >"$OUTPUT_FILE"
  RUN_STATUS=0
  RUN_OUT="$(cd "$REPO" && env "GITHUB_OUTPUT=$OUTPUT_FILE" "$CHECK" 2>&1)" || RUN_STATUS=$?
}

# Asserts the run succeeded and wrote exactly $2 to $GITHUB_OUTPUT.
assert_output() {
  if [ "$RUN_STATUS" -ne 0 ]; then
    fail "$1 (the script exits 0)" "  exit status: $RUN_STATUS
$RUN_OUT"
  fi
  assert_eq "$1" "$2" "$(cat "$OUTPUT_FILE")"
}

# --- Version unchanged. --------------------------------------------------------

new_repo unchanged
printf '// touched\n' >>"$REPO/backend/crates/delta-server/src/main.rs"
commit_change
run_check unchanged
assert_output "an unchanged version reports changed=false" "changed=false"

# --- Version bumped. -----------------------------------------------------------

new_repo bumped
write_root_manifest 0.2.0 ""
commit_change
run_check bumped
assert_output "a bumped version reports changed=true and the new version" \
  "changed=true
version=0.2.0"

# --- HEAD adds an inherited workspace dependency, version unchanged. -----------

# HEAD~1's root manifest lacks the [workspace.dependencies] entry that HEAD's
# delta-server inherits, so the previous version must be read from HEAD~1's
# tree alone: pairing the old root with the new member manifest fails to load.
new_repo inherited-dep
write_root_manifest 0.1.0 yes
write_server_manifest yes
commit_change
run_check inherited-dep
assert_output "a new inherited workspace dependency alone reports changed=false" \
  "changed=false"

# --- The same, together with a version bump. -----------------------------------

new_repo inherited-dep-bumped
write_root_manifest 0.2.0 yes
write_server_manifest yes
commit_change
run_check inherited-dep-bumped
assert_output "a new inherited workspace dependency with a bump reports changed=true" \
  "changed=true
version=0.2.0"

# --- The working tree is left as it was. ---------------------------------------

assert_eq "the run leaves the scratch working tree clean" "" \
  "$(git -C "$REPO" status --porcelain)"

printf 'all check-version-change tests passed\n'
