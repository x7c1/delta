#!/usr/bin/env bash
#
# Tests for scripts/check-workflow-pins.sh — the check that every action a
# workflow uses is pinned to a commit SHA.
#
# Usage: bash scripts/tests/check-workflow-pins.test.sh
#
# Each case writes a throwaway workflows directory under `mktemp -d` and runs
# the checker against it (its only argument), so the verdicts do not depend on
# the state of the repository's own workflows. Needs only bash and coreutils.
#
# The script exits non-zero on the first failed assertion.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECK="$(cd "$SCRIPT_DIR/.." && pwd)/check-workflow-pins.sh"

[ -x "$CHECK" ] || {
  printf 'FAIL - check-workflow-pins script is not executable: %s\n' "$CHECK" >&2
  exit 1
}

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/delta-workflow-pins-test.XXXXXX")"
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

assert_contains() {
  case "$2" in
    *"$3"*) pass "$1" ;;
    *) fail "$1" "  expected to find $(printf '%q' "$3") in:
$2" ;;
  esac
}

assert_lacks() {
  case "$2" in
    *"$3"*) fail "$1" "  expected NOT to find $(printf '%q' "$3") in:
$2" ;;
    *) pass "$1" ;;
  esac
}

SHA=11d5960a326750d5838078e36cf38b85af677262

# Writes a fresh workflows directory named $1 and prints its path; the
# workflow body comes from stdin.
workflows() {
  local dir="$WORK_DIR/$1"
  mkdir -p "$dir"
  cat >"$dir/ci.yml"
  printf '%s\n' "$dir"
}

OUT=""
STATUS=0
run_check() {
  STATUS=0
  OUT="$("$CHECK" "$@" 2>&1)" || STATUS=$?
}

# --- A tag reference fails, naming the file, the line and the reference. -------

dir="$(workflows tag <<'YAML'
name: CI
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Toolchain
        uses: dtolnay/rust-toolchain@stable
YAML
)"
run_check "$dir"
assert_eq "a workflow with tag references fails" 1 "$STATUS"
assert_contains "the step-item reference is named with its line" "$OUT" \
  "$dir/ci.yml:7: actions/checkout@v4 is not pinned to a full commit SHA"
assert_contains "the key-only reference is named with its line" "$OUT" \
  "$dir/ci.yml:9: dtolnay/rust-toolchain@stable is not pinned to a full commit SHA"

# --- SHA references pass; local reusable workflows are exempt. ---------------

dir="$(workflows sha <<YAML
name: CI
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@${SHA} # v4.4.0
      - name: Restore
        uses: "actions/cache/restore@${SHA}" # v4.3.0
  bundle:
    uses: ./.github/workflows/bundle.yml
YAML
)"
run_check "$dir"
assert_eq "a workflow pinning every action to a SHA passes" 0 "$STATUS"
assert_contains "the pass counts the pinned references, not the local one" "$OUT" \
  "2 action reference(s)"

# --- A SHA without its version comment fails. -------------------------------

dir="$(workflows no-comment <<YAML
jobs:
  build:
    steps:
      - uses: actions/checkout@${SHA}
YAML
)"
run_check "$dir"
assert_eq "a SHA without a version comment fails" 1 "$STATUS"
assert_contains "the reference lacking the comment is named" "$OUT" \
  "$dir/ci.yml:4: actions/checkout@${SHA} carries no '# <version>' comment"

# --- A short SHA is not a pin. ------------------------------------------------

dir="$(workflows short <<'YAML'
jobs:
  build:
    steps:
      - uses: actions/checkout@11d5960 # v4.4.0
YAML
)"
run_check "$dir"
assert_eq "an abbreviated SHA fails" 1 "$STATUS"
assert_lacks "an abbreviated SHA is not reported as missing its comment" "$OUT" \
  "carries no"

# --- Usage errors. ----------------------------------------------------------

run_check "$WORK_DIR/does-not-exist"
assert_eq "a missing directory is a usage error" 2 "$STATUS"
assert_contains "the missing directory is named" "$OUT" "no such directory"

printf 'all check-workflow-pins tests passed\n'
