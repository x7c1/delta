#!/usr/bin/env bash
#
# Tests for scripts/gen-check.sh — the wire-bindings freshness check.
#
# Usage: bash scripts/tests/gen-check.test.sh
#
# The check's contract is "the bindings on disk are what the contract generates
# now", independent of git and without writing the working tree. Running the
# real `export-ts` here would only ever exercise the one state the repo is in,
# so both of the script's test seams are used: DELTA_EXPORT_TS points at a stub
# generator that copies a fixture tree, and DELTA_WIRE_GEN_DIR points at a
# throwaway "committed" directory inside a scratch git repository whose HEAD
# deliberately disagrees with its working tree. TMPDIR is pointed at an empty
# directory so every run can be checked for leaving its scratch dir behind.
# Needs only bash, coreutils, diff and git.
#
# The script exits non-zero on the first failed assertion: a test that limps to
# the end reporting "3 failures" is a test nobody reads.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GEN_CHECK="$(cd "$SCRIPT_DIR/.." && pwd)/gen-check.sh"

[ -x "$GEN_CHECK" ] || {
  printf 'FAIL - gen-check script is not executable: %s\n' "$GEN_CHECK" >&2
  exit 1
}
command -v git >/dev/null 2>&1 || {
  printf 'FAIL - git not found on PATH; it builds the uncommitted-change fixture\n' >&2
  exit 1
}

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/delta-gen-check-test.XXXXXX")"
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

# --- The fixture the stub generator emits: "what the contract generates now". ---

FIXTURE="$WORK_DIR/fixture"
mkdir -p "$FIXTURE"
printf 'export type Session = { id: string; title: string };\n' >"$FIXTURE/Session.ts"
printf "export const EVENT_KINDS = ['turn_started'] as const;\n" >"$FIXTURE/event-kinds.ts"

BIN_DIR="$WORK_DIR/bin"
mkdir -p "$BIN_DIR"

# Mirrors export-ts's interface: one argument, the output directory, which it
# creates. The file $WORK_DIR/generator-fails makes it exit non-zero after
# writing, the way a panic half-way through the real exporter would.
cat >"$BIN_DIR/export-ts" <<STUB
#!/usr/bin/env bash
set -euo pipefail
[ \$# -eq 1 ] || { echo "stub export-ts: expected one argument, got \$#" >&2; exit 2; }
mkdir -p "\$1"
cp -R "$FIXTURE/." "\$1/"
if [ -e "$WORK_DIR/generator-fails" ]; then
  echo "stub export-ts: simulated failure" >&2
  exit 3
fi
STUB
chmod +x "$BIN_DIR/export-ts"

# --- The "repository": HEAD holds an older contract than the working tree. ------

REPO="$WORK_DIR/repo"
COMMITTED_DIR="$REPO/generated"
SCRATCH_TMP="$WORK_DIR/tmp"
mkdir -p "$SCRATCH_TMP"

git init -q "$REPO" || fail "fixture: git init"
mkdir -p "$COMMITTED_DIR"
printf 'export type Session = { id: string };\n' >"$COMMITTED_DIR/Session.ts"
cp "$FIXTURE/event-kinds.ts" "$COMMITTED_DIR/event-kinds.ts"
git -C "$REPO" add -A || fail "fixture: git add"
git -C "$REPO" -c user.name=test -c user.email=test@example.invalid \
  -c commit.gpgsign=false commit -q -m "bindings of the previous contract" \
  || fail "fixture: git commit"

# A developer changed the contract and ran `make gen`: the working tree now
# matches the generator, but the change is not committed.
cp "$FIXTURE/Session.ts" "$COMMITTED_DIR/Session.ts"

# --- Harness. -------------------------------------------------------------------

OUT=""
STATUS=0

# Every file under the committed dir with its checksum, plus git's view of it —
# compared before and after each run to prove the check writes nothing.
snapshot() {
  (cd "$COMMITTED_DIR" && find . -type f -exec cksum {} + | LC_ALL=C sort)
  git -C "$REPO" status --porcelain
}

# Runs the check against the stub and the throwaway dir; extra arguments are
# forwarded. Every run also asserts that it wrote nothing and cleaned up.
run_gen_check() {
  local before after
  before="$(snapshot)"
  STATUS=0
  OUT="$(
    env "DELTA_EXPORT_TS=$BIN_DIR/export-ts" \
      "DELTA_WIRE_GEN_DIR=$COMMITTED_DIR" \
      "TMPDIR=$SCRATCH_TMP" \
      "$GEN_CHECK" "$@" 2>&1
  )" || STATUS=$?
  after="$(snapshot)"
  assert_eq "  the run leaves the bindings and git state untouched" "$before" "$after"
  assert_eq "  the run removes its scratch directory" "" "$(ls -A "$SCRATCH_TMP")"
}

# --- Fresh but uncommitted bindings pass. ---------------------------------------

assert_contains "fixture: the working tree differs from HEAD" \
  "$(git -C "$REPO" status --porcelain)" "generated/Session.ts"
run_gen_check
assert_eq "bindings that match the generator pass although they are not committed" 0 "$STATUS"
assert_contains "a pass says the bindings are fresh" "$OUT" "generated wire bindings are fresh"

# --- A stale file fails, with the diff and the fix. -----------------------------

printf 'export type Session = { id: number };\n' >"$COMMITTED_DIR/Session.ts"
run_gen_check
assert_eq "a stale generated file fails the check" 1 "$STATUS"
assert_contains "the failure prints the stale line" "$OUT" \
  "-export type Session = { id: number };"
assert_contains "the failure prints the generated line" "$OUT" \
  "+export type Session = { id: string; title: string };"
assert_contains "the failure says how to fix it" "$OUT" "run 'make gen' and commit the result"
assert_lacks "a stale file gets no hint about orphaned files" "$OUT" "no longer generated"
cp "$FIXTURE/Session.ts" "$COMMITTED_DIR/Session.ts"

# --- A clean checkout of stale bindings fails too (CI's case). ------------------

# On a clean checkout the files on disk are HEAD's, so the same comparison is
# what catches bindings that were regenerated but never committed.
git -C "$REPO" checkout -q -- generated/Session.ts
assert_eq "fixture: the working tree is clean" "" "$(git -C "$REPO" status --porcelain)"
run_gen_check
assert_eq "a clean checkout of stale bindings fails the check" 1 "$STATUS"
assert_contains "and names the stale file" "$OUT" "Session.ts"
cp "$FIXTURE/Session.ts" "$COMMITTED_DIR/Session.ts"

# --- Files the generator adds or no longer emits fail. --------------------------

printf 'export type Gone = never;\n' >"$COMMITTED_DIR/Gone.ts"
run_gen_check
assert_eq "a committed file the generator no longer emits fails the check" 1 "$STATUS"
assert_contains "the orphaned file is named" "$OUT" "Only in $COMMITTED_DIR: Gone.ts"
assert_contains "the failure says what to do with it" "$OUT" "no longer generated: delete it"
rm "$COMMITTED_DIR/Gone.ts"

rm "$COMMITTED_DIR/event-kinds.ts"
run_gen_check
assert_eq "a generated file missing from the committed dir fails the check" 1 "$STATUS"
assert_contains "the missing file is named" "$OUT" "event-kinds.ts"
cp "$FIXTURE/event-kinds.ts" "$COMMITTED_DIR/event-kinds.ts"

run_gen_check
assert_eq "restoring the files makes the check pass again" 0 "$STATUS"

# --- A failing generator is an error, not a verdict. ----------------------------

: >"$WORK_DIR/generator-fails"
run_gen_check
assert_eq "a failing generator fails the check" 1 "$STATUS"
assert_contains "the failure blames the generator" "$OUT" "generating the wire bindings failed"
assert_lacks "and does not claim the bindings are stale" "$OUT" "are stale"
rm "$WORK_DIR/generator-fails"

# --- Bad invocations fail loudly. -----------------------------------------------

run_gen_check --nope
assert_eq "an argument is rejected" 1 "$STATUS"
assert_contains "with a usage line" "$OUT" "takes no arguments"

STATUS=0
OUT="$(
  env "DELTA_EXPORT_TS=$BIN_DIR/export-ts" \
    "DELTA_WIRE_GEN_DIR=$WORK_DIR/no-such-dir" \
    "TMPDIR=$SCRATCH_TMP" \
    "$GEN_CHECK" 2>&1
)" || STATUS=$?
assert_eq "a missing bindings directory fails the check" 1 "$STATUS"
assert_contains "and is named" "$OUT" "bindings directory not found"

printf '\nAll assertions passed.\n'
exit 0
