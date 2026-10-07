#!/usr/bin/env bash
#
# Tests for scripts/sweep-test-residue.sh — against a planted temp tree, never
# the host's real tmux sockets or temp directory.
#
# Usage: bash scripts/tests/sweep-test-residue.test.sh
#
# TMUX_TMPDIR and TMPDIR point into a `mktemp -d`, and a stub `tmux` first on
# PATH records every argument list it is given (and fails, like a real
# `kill-server` against a stale socket file). The planted sockets are plain
# files, so nothing here needs a real tmux server.
#
# The script exits non-zero on the first failed assertion.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SWEEP="$(cd "$SCRIPT_DIR/.." && pwd)/sweep-test-residue.sh"

[ -x "$SWEEP" ] || { printf 'FAIL - sweep script is not executable: %s\n' "$SWEEP" >&2; exit 1; }

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/delta-sweep-test.XXXXXX")"
# Processes the test starts in the background, stopped on exit even when an
# assertion fails first.
SPAWNED_PIDS=""
cleanup() {
  local pid
  for pid in $SPAWNED_PIDS; do kill "$pid" 2>/dev/null; done
  rm -rf "$WORK_DIR"
}
trap cleanup EXIT

BIN_DIR="$WORK_DIR/bin"
TMUX_LOG="$WORK_DIR/tmux-calls"
mkdir -p "$BIN_DIR"
: >"$TMUX_LOG"
cat >"$BIN_DIR/tmux" <<STUB
#!/usr/bin/env bash
printf '%s\n' "\$*" >>"$TMUX_LOG"
exit 1
STUB
chmod +x "$BIN_DIR/tmux"

FAKE_TMUX_TMPDIR="$WORK_DIR/tmux-tmp"
FAKE_TMPDIR="$WORK_DIR/tmp"
SOCKETS="$FAKE_TMUX_TMPDIR/tmux-$(id -u)"
mkdir -p "$SOCKETS" "$FAKE_TMPDIR"

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

assert_exists() { if [ -e "$2" ]; then pass "$1"; else fail "$1" "  missing: $2"; fi; }
assert_missing() { if [ -e "$2" ]; then fail "$1" "  still exists: $2"; else pass "$1"; fi; }

assert_contains() {
  case "$2" in
    *"$3"*) pass "$1" ;;
    *) fail "$1" "  expected to find $(printf '%q' "$3") in:
$2" ;;
  esac
}

assert_not_contains() {
  case "$2" in
    *"$3"*) fail "$1" "  expected NOT to find $(printf '%q' "$3") in:
$2" ;;
    *) pass "$1" ;;
  esac
}

# A pid that is certainly dead: a child that has already been reaped.
sh -c 'exit 0' &
DEAD_PID=$!
wait "$DEAD_PID"
kill -0 "$DEAD_PID" 2>/dev/null && fail "the reaped child $DEAD_PID is still alive"
LIVE_PID=$$

RUN_STATUS=0
RUN_OUT=""
run_sweep() {
  RUN_STATUS=0
  RUN_OUT="$(
    env "PATH=$BIN_DIR:$PATH" \
      "TMUX_TMPDIR=$FAKE_TMUX_TMPDIR" \
      "TMPDIR=$FAKE_TMPDIR" \
      "$SWEEP" "$@" 2>&1
  )" || RUN_STATUS=$?
}

# Plants a run directory with an optional owner pid and a fixed mtime.
plant_run() {
  mkdir -p "$1"
  printf 'log\n' >"$1/server.log"
  [ -n "$2" ] && printf '%s\n' "$2" >"$1/owner.pid"
  touch -t "$3" "$1"
}

# --- Nothing to sweep is not an error. -----------------------------------------

run_sweep e2e-fake
assert_eq "an empty tree exits 0" 0 "$RUN_STATUS"
assert_eq "an empty tree prints nothing" "" "$RUN_OUT"
assert_eq "an empty tree calls no tmux" "" "$(cat "$TMUX_LOG")"

# --- The planted tree. ---------------------------------------------------------

DEAD_SOCKET="$SOCKETS/delta-e2e-fake-$DEAD_PID"
LIVE_SOCKET="$SOCKETS/delta-e2e-fake-$LIVE_PID"
DEV_SOCKET="$SOCKETS/io.github.x7c1.delta.dev"
DEFAULT_SOCKET="$SOCKETS/default"
OTHER_CONTEXT_SOCKET="$SOCKETS/delta-e2e-real-$DEAD_PID"
for socket in "$DEAD_SOCKET" "$LIVE_SOCKET" "$DEV_SOCKET" "$DEFAULT_SOCKET" "$OTHER_CONTEXT_SOCKET"; do
  : >"$socket"
done

OLDER_DEAD="$FAKE_TMPDIR/delta-e2e-fake.older"
NEWER_DEAD="$FAKE_TMPDIR/delta-e2e-fake.newer"
NO_OWNER="$FAKE_TMPDIR/delta-e2e-fake.noowner"
LIVE_RUN="$FAKE_TMPDIR/delta-e2e-fake.live"
OTHER_CONTEXT_RUN="$FAKE_TMPDIR/delta-e2e-real.dead"
# The live run is the OLDEST, so it would be removed if it were taken for dead;
# the ownerless one is older than the newer dead run, so it goes if it counts
# as dead.
plant_run "$LIVE_RUN" "$LIVE_PID" 201801010000
plant_run "$NO_OWNER" "" 201901010000
plant_run "$OLDER_DEAD" "$DEAD_PID" 202001010000
plant_run "$NEWER_DEAD" "$DEAD_PID" 202101010000
plant_run "$OTHER_CONTEXT_RUN" "$DEAD_PID" 201701010000

run_sweep e2e-fake
TMUX_CALLS="$(cat "$TMUX_LOG")"

assert_eq "the sweep exits 0" 0 "$RUN_STATUS"

assert_contains "the dead-owner socket's server is killed" "$TMUX_CALLS" \
  "-L delta-e2e-fake-$DEAD_PID kill-server"
assert_missing "the dead-owner socket file is unlinked" "$DEAD_SOCKET"
assert_exists "the live-owner socket survives" "$LIVE_SOCKET"
assert_not_contains "the live-owner socket is never passed to tmux" "$TMUX_CALLS" \
  "delta-e2e-fake-$LIVE_PID"
assert_exists "the dev socket survives" "$DEV_SOCKET"
assert_not_contains "the dev socket is never passed to tmux" "$TMUX_CALLS" "io.github.x7c1.delta"
assert_exists "the default socket survives" "$DEFAULT_SOCKET"
assert_exists "another context's socket survives" "$OTHER_CONTEXT_SOCKET"
assert_eq "tmux is called exactly once" 1 "$(grep -c . "$TMUX_LOG")"

assert_missing "the older dead run directory is removed" "$OLDER_DEAD"
assert_exists "the newest dead run directory is kept" "$NEWER_DEAD"
assert_exists "the kept directory keeps its evidence" "$NEWER_DEAD/server.log"
assert_missing "a directory without owner.pid counts as dead" "$NO_OWNER"
assert_exists "the live-owner run directory survives" "$LIVE_RUN"
assert_exists "another context's run directory survives" "$OTHER_CONTEXT_RUN"

assert_contains "the socket removal is reported" "$RUN_OUT" "removed socket $DEAD_SOCKET"
assert_contains "the kept directory is reported" "$RUN_OUT" "kept $NEWER_DEAD"
assert_contains "a removal is reported" "$RUN_OUT" "removed run directory $OLDER_DEAD"

# --- A second sweep is idempotent. ---------------------------------------------

: >"$TMUX_LOG"
run_sweep e2e-fake
assert_eq "a repeated sweep exits 0" 0 "$RUN_STATUS"
assert_exists "a repeated sweep still keeps the newest dead run" "$NEWER_DEAD"
assert_eq "a repeated sweep calls no tmux" "" "$(cat "$TMUX_LOG")"

# --- Directory-less contexts sweep sockets only. -------------------------------

: >"$TMUX_LOG"
CANARY_DEAD="$SOCKETS/delta-canary-prompt_turn-$DEAD_PID"
CANARY_LIVE="$SOCKETS/delta-canary-prompt_turn-$LIVE_PID"
: >"$CANARY_DEAD"
: >"$CANARY_LIVE"
run_sweep canary
assert_eq "the canary sweep exits 0" 0 "$RUN_STATUS"
assert_missing "a dead canary socket is unlinked" "$CANARY_DEAD"
assert_exists "a live canary socket survives" "$CANARY_LIVE"
assert_exists "the canary sweep leaves e2e-fake runs alone" "$NEWER_DEAD"
assert_exists "the canary sweep leaves the dev socket alone" "$DEV_SOCKET"

# --- A dead run's orphaned delta-server is killed, and nothing else. ------------

# Started detached (not this shell's children), so a killed one is reaped at
# once and `kill -0` stops seeing it. The orphan is a shell script named
# `delta-server`, which is the name `ps -o comm=` reports for it on Linux and
# macOS. It waits in the shell itself (a builtin `read` on a FIFO it holds
# open) rather than running `sleep`: a child would outlive the `kill -9`, and a
# symlink to `sleep` does not work with a multicall coreutils (uutils, the
# default on recent Ubuntu), which refuses to run under another name.
cat >"$BIN_DIR/delta-server" <<'SH'
#!/bin/sh
fifo="$0.fifo.$$"
mkfifo "$fifo"
exec 3<>"$fifo"
rm -f "$fifo"
read -r _ <&3
SH
chmod +x "$BIN_DIR/delta-server"
ORPHAN_PID="$("$BIN_DIR/delta-server" >/dev/null 2>&1 & echo $!)"
BYSTANDER_PID="$(sleep 60 >/dev/null 2>&1 & echo $!)"
SPAWNED_PIDS="$ORPHAN_PID $BYSTANDER_PID"
ORPHAN_RUN="$FAKE_TMPDIR/delta-e2e-fake.orphan"
plant_run "$ORPHAN_RUN" "$DEAD_PID" 202201010000
printf '%s\n%s\n' "$ORPHAN_PID" "$BYSTANDER_PID" >"$ORPHAN_RUN/server.pids"

run_sweep e2e-fake
assert_eq "the orphan sweep exits 0" 0 "$RUN_STATUS"
for _ in $(seq 1 50); do
  kill -0 "$ORPHAN_PID" 2>/dev/null || break
  sleep 0.1
done
if kill -0 "$ORPHAN_PID" 2>/dev/null; then
  fail "a dead run's live delta-server is killed" "  still alive: $ORPHAN_PID"
else
  pass "a dead run's live delta-server is killed"
fi
if kill -0 "$BYSTANDER_PID" 2>/dev/null; then
  pass "a recorded pid that is not a delta-server survives"
else
  fail "a recorded pid that is not a delta-server survives" "  killed: $BYSTANDER_PID"
fi
assert_contains "the orphan kill is reported" "$RUN_OUT" "killed orphaned delta-server $ORPHAN_PID"
assert_exists "the run directory listing the orphan is kept as the newest" "$ORPHAN_RUN"
assert_missing "the previously kept directory is now older and removed" "$NEWER_DEAD"

# --- An unknown context is a usage error. --------------------------------------

run_sweep nonsense
assert_eq "an unknown context exits 2" 2 "$RUN_STATUS"
assert_exists "an unknown context touches nothing" "$OTHER_CONTEXT_SOCKET"

printf 'all sweep-test-residue tests passed\n'
