#!/usr/bin/env bash
#
# Tests for scripts/e2e-real-gate.sh — the gate logic only, never a real suite.
#
# Usage: bash scripts/tests/e2e-real-gate.test.sh
#
# Everything the gate touches is faked: stub `claude`/`codex` binaries on a
# temp PATH print whatever version the test wants, XDG_STATE_HOME points at a
# temp state dir, and E2E_REAL_CMD replaces the suite with a stub that appends
# the provider it was invoked for (E2E_REAL_GATE_PROVIDER) to a witness file.
# So the whole file spends no quota and needs nothing but bash, coreutils and
# the stubs it writes itself — no flock, no GNU-only date/head flags.
#
# The script exits non-zero on the first failed assertion: a gate test that
# limps to the end reporting "3 failures" is a gate test nobody reads.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GATE="$(cd "$SCRIPT_DIR/.." && pwd)/e2e-real-gate.sh"

[ -x "$GATE" ] || { printf 'FAIL - gate script is not executable: %s\n' "$GATE" >&2; exit 1; }

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/delta-gate-test.XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT

BIN_DIR="$WORK_DIR/bin"
mkdir -p "$BIN_DIR"

# The stub CLIs read their version from a file so a test can bump one CLI
# between ticks the way a real auto-update would.
for provider in claude codex; do
  printf 'v1.0.0\n' >"$WORK_DIR/$provider.version"
  cat >"$BIN_DIR/$provider" <<EOF
#!/usr/bin/env bash
cat "$WORK_DIR/$provider.version"
EOF
  chmod +x "$BIN_DIR/$provider"
done

# The stub suite: records the provider it ran for, and fails for whichever
# providers \$WORK_DIR/fail-for lists.
WITNESS="$WORK_DIR/ran"
cat >"$WORK_DIR/suite-stub.sh" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\${E2E_REAL_GATE_PROVIDER:-<unset>}" >>"$WITNESS"
if grep -qx "\${E2E_REAL_GATE_PROVIDER:-}" "$WORK_DIR/fail-for" 2>/dev/null; then
  echo "stub suite failing on purpose"
  exit 3
fi
exit 0
EOF
chmod +x "$WORK_DIR/suite-stub.sh"
: >"$WORK_DIR/fail-for"

# Stub the notifiers too: they shadow the host's real ones (BIN_DIR is first on
# PATH), so a failing-provider test records what would have been shown instead
# of popping a notification on the developer's desktop.
NOTIFY_LOG="$WORK_DIR/notifications"
: >"$NOTIFY_LOG"
for tool in notify-send osascript; do
  cat >"$BIN_DIR/$tool" <<EOF
#!/usr/bin/env bash
printf '$tool %s\n' "\$*" >>"$NOTIFY_LOG"
EOF
  chmod +x "$BIN_DIR/$tool"
done
HOST_HAS_NOTIFY_SEND=""
command -v notify-send >/dev/null 2>&1 && HOST_HAS_NOTIFY_SEND=1

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

assert_file_missing() {
  if [ -e "$2" ]; then
    fail "$1" "  unexpectedly exists: $2"
  else
    pass "$1"
  fi
}

# --- Harness. ------------------------------------------------------------------

STATE_HOME=""
TICK_OUT=""
TICK_STATUS=0

reset_state() {
  STATE_HOME="$WORK_DIR/state-$1"
  rm -rf "$STATE_HOME"
  mkdir -p "$STATE_HOME"
  : >"$WORK_DIR/fail-for"
  : >"$WITNESS"
  : >"$NOTIFY_LOG"
  printf 'v1.0.0\n' >"$WORK_DIR/claude.version"
  printf 'v1.0.0\n' >"$WORK_DIR/codex.version"
}

# Runs one gate tick with the stubs in front of PATH. Extra arguments are
# `NAME=value` env overrides for that tick.
tick() {
  TICK_STATUS=0
  TICK_OUT="$(
    env "PATH=$BIN_DIR:$PATH" \
      "XDG_STATE_HOME=$STATE_HOME" \
      "E2E_REAL_CMD=$WORK_DIR/suite-stub.sh" \
      "$@" \
      "$GATE" 2>&1
  )" || TICK_STATUS=$?
}

# Which providers the stub suite ran during the last tick, in order, on one line.
ran_providers() { tr '\n' ' ' <"$WITNESS" | sed -e 's/ $//'; }

state_field() { sed -n "s/^$2=//p" "$STATE_HOME/delta/e2e-real/$1/last-attempt"; }

# Rewrites a provider's recorded attempt epoch so the debounce sees it as old.
age_attempt() {
  state_file="$STATE_HOME/delta/e2e-real/$1/last-attempt"
  old_epoch="$(sed -n 's/^epoch=//p' "$state_file")"
  new_epoch=$((old_epoch - $2))
  sed -e "s/^epoch=.*/epoch=$new_epoch/" "$state_file" >"$state_file.tmp"
  mv "$state_file.tmp" "$state_file"
}

# --- Tick 1: a fresh host runs both providers. ----------------------------------

reset_state basic
tick
assert_eq "first tick exits 0" 0 "$TICK_STATUS"
assert_eq "first tick runs both providers in order" "claude codex" "$(ran_providers)"
assert_eq "claude records success" success "$(state_field claude result)"
assert_eq "codex records success" success "$(state_field codex result)"
assert_eq "claude records the stub version" v1.0.0 "$(state_field claude version)"
assert_eq "codex records the stub version" v1.0.0 "$(state_field codex version)"
assert_contains "the summary names claude's outcome" "$TICK_OUT" "claude: ran: success"
assert_contains "the summary names codex's outcome" "$TICK_OUT" "codex: ran: success"

# --- Tick 2: nothing changed, so nothing runs. ----------------------------------

: >"$WITNESS"
tick
assert_eq "unchanged tick exits 0" 0 "$TICK_STATUS"
assert_eq "unchanged tick runs no suite" "" "$(ran_providers)"
assert_contains "claude is skipped as unchanged" "$TICK_OUT" "claude: skipped (version unchanged"
assert_contains "codex is skipped as unchanged" "$TICK_OUT" "codex: skipped (version unchanged"

# --- Tick 3: codex updated, but the debounce defers it to a later tick. ---------

: >"$WITNESS"
printf 'v1.1.0\n' >"$WORK_DIR/codex.version"
tick
assert_eq "debounced tick exits 0" 0 "$TICK_STATUS"
assert_eq "debounced tick runs no suite" "" "$(ran_providers)"
assert_contains "codex is deferred to a later tick" "$TICK_OUT" "runs on a later tick"
assert_contains "claude is still evaluated and skipped" "$TICK_OUT" "claude: skipped (version unchanged"
assert_eq "codex keeps the old recorded version while deferred" v1.0.0 "$(state_field codex version)"

# --- Tick 4: once the debounce window passes, the update runs. ------------------

: >"$WITNESS"
age_attempt codex $((25 * 60 * 60))
tick
assert_eq "post-debounce tick exits 0" 0 "$TICK_STATUS"
assert_eq "post-debounce tick runs codex only" "codex" "$(ran_providers)"
assert_eq "codex records the new version" v1.1.0 "$(state_field codex version)"

# --- A failing provider: non-zero exit, the other provider still evaluated. -----

reset_state failure
tick
assert_eq "seed tick exits 0" 0 "$TICK_STATUS"

: >"$WITNESS"
printf 'codex\n' >"$WORK_DIR/fail-for"
printf 'v2.0.0\n' >"$WORK_DIR/codex.version"
age_attempt codex $((25 * 60 * 60))
tick
assert_eq "a failing provider makes the tick exit non-zero" 3 "$TICK_STATUS"
assert_eq "only codex ran" "codex" "$(ran_providers)"
assert_eq "codex records the failure and its exit code" "failure (exit 3)" "$(state_field codex result)"
assert_contains "the FAILURE line names the provider" "$TICK_OUT" "FAILURE: real-codex canary suite failed (exit 3)"
assert_contains "claude's gate is still evaluated" "$TICK_OUT" "claude: skipped (version unchanged"
assert_contains "the summary reports codex's failure" "$TICK_OUT" "codex: ran: failure (exit 3)"
assert_eq "claude's own state is untouched by codex's failure" success "$(state_field claude result)"
assert_contains "the failure fires a desktop notification" "$(cat "$NOTIFY_LOG")" \
  "notify-send -u critical Delta canary FAILED real-codex suite failed"

# Without notify-send (the stock macOS case) the same failure falls back to
# osascript. Only assertable where the host has no real notify-send to find
# once the stub is out of the way.
: >"$NOTIFY_LOG"
rm -f "$BIN_DIR/notify-send"
printf 'v2.1.0\n' >"$WORK_DIR/codex.version"
age_attempt codex $((25 * 60 * 60))
tick
assert_eq "the notifier fallback tick still exits non-zero" 3 "$TICK_STATUS"
if [ -n "$HOST_HAS_NOTIFY_SEND" ]; then
  pass "osascript fallback check skipped (this host has a real notify-send)"
else
  assert_contains "the failure falls back to osascript" "$(cat "$NOTIFY_LOG")" \
    "osascript -e display notification"
fi

# A red canary is not retried until the CLI updates again, so every later tick
# must keep saying it is red — otherwise the FAILURE line scrolls away and the
# hourly log reads green.
: >"$WITNESS"
tick
assert_eq "the tick after a red canary exits 0" 0 "$TICK_STATUS"
assert_eq "the tick after a red canary runs nothing" "" "$(ran_providers)"
assert_contains "a red last attempt stays visible on later ticks" "$TICK_OUT" \
  "codex: that attempt did not pass (failure (exit 3)) and is not retried until codex updates again"
assert_contains "the summary carries the unresolved failure" "$TICK_OUT" \
  "codex: skipped (version unchanged; last attempt: failure (exit 3))"
assert_contains "a green provider's skip line stays plain" "$TICK_OUT" \
  "claude: skipped (version unchanged since the last attempt: v1.0.0)"

# --- Legacy state migration: the root state file was always claude's. -----------

reset_state legacy
legacy_dir="$STATE_HOME/delta/e2e-real"
mkdir -p "$legacy_dir"
cat >"$legacy_dir/last-attempt" <<'EOF'
version=v1.0.0
epoch=1000000000
date=2001-09-09T01:46:40Z
result=success
log=/dev/null
EOF
tick
assert_eq "migrating tick exits 0" 0 "$TICK_STATUS"
assert_file_missing "the legacy state file is gone" "$legacy_dir/last-attempt"
assert_eq "the legacy state landed in claude/" v1.0.0 "$(state_field claude version)"
assert_eq "the migrated state is honoured: claude does not re-run" "codex" "$(ran_providers)"
assert_contains "the migration is logged" "$TICK_OUT" "migrated the pre-provider-aware state file"

# --- E2E_REAL_GATE_PROVIDERS restricts the provider list. -----------------------

reset_state subset
tick E2E_REAL_GATE_PROVIDERS=codex
assert_eq "restricted tick exits 0" 0 "$TICK_STATUS"
assert_eq "only the requested provider runs" "codex" "$(ran_providers)"
assert_file_missing "no state is written for the excluded provider" \
  "$STATE_HOME/delta/e2e-real/claude/last-attempt"

# --- An unknown provider is an error, not a silent skip. ------------------------

reset_state unknown-provider
tick E2E_REAL_GATE_PROVIDERS=claud
assert_eq "an unknown provider fails the tick" 1 "$TICK_STATUS"
assert_eq "an unknown provider runs nothing" "" "$(ran_providers)"
assert_contains "the unknown provider is named" "$TICK_OUT" "unknown provider: claud"

# --- A provider whose binary is missing is skipped, the other one still runs. ---

reset_state missing-binary
tick "DELTA_CODEX_BIN=$WORK_DIR/no-such-codex"
assert_eq "missing-binary tick exits 0" 0 "$TICK_STATUS"
assert_eq "the present provider still runs" "claude" "$(ran_providers)"
assert_contains "the missing provider is skipped quietly" "$TICK_OUT" "codex: skipped (codex binary not found"

# --- The lock is honoured: a held lock skips the whole tick. --------------------

reset_state locked
mkdir -p "$STATE_HOME/delta/e2e-real/lock.d"
printf '%s\n' "$$" >"$STATE_HOME/delta/e2e-real/lock.d/pid"
# The fallback lock is only reachable on a host without flock; on a host with
# flock the gate uses the lock FILE and ignores the directory, so this
# assertion only applies where the fallback is the guard.
if command -v flock >/dev/null 2>&1; then
  pass "lock directory check skipped (flock is the guard on this host)"
else
  tick
  assert_eq "a tick with the lock held exits 0" 0 "$TICK_STATUS"
  assert_eq "a tick with the lock held runs nothing" "" "$(ran_providers)"
  assert_contains "the held lock is reported" "$TICK_OUT" "another real-agent suite run is in flight"

  # A lock left behind by a dead process must not wedge the gate forever.
  printf '%s\n' 999999 >"$STATE_HOME/delta/e2e-real/lock.d/pid"
  tick
  assert_eq "a stale lock is reclaimed" "claude codex" "$(ran_providers)"
  assert_file_missing "the lock directory is released on exit" \
    "$STATE_HOME/delta/e2e-real/lock.d"
fi

printf '\nAll assertions passed.\n'
exit 0
