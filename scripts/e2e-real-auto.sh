#!/usr/bin/env bash
#
# e2e-real-auto.sh — gated automatic trigger for the real-claude canary suite.
#
# Meant to be invoked by a periodic driver (the systemd user timer under
# scripts/systemd/, or a cron line — see docs/guides/development.md,
# "Automatic canary trigger"). Each tick it runs `make e2e-real` only when
# BOTH hold:
#
#   (a) the installed `claude` CLI version differs from the version recorded
#       at the last attempt, and
#   (b) at least 24 hours have passed since the last attempt.
#
# Rationale: claude auto-updates frequently (sometimes several times a day)
# and the suite consumes a handful of real subscription turns per run. The
# gate caps automatic spend at one run per day, spends nothing on days
# without an update, and never misses an update (the version comparison
# catches up on a later tick once the debounce window has passed).
#
# The debounce is on the ATTEMPT, not on success: a red canary usually means
# real upstream drift, and auto-retrying it hourly would burn quota without
# producing new information. Failures are loud instead: non-zero exit, a
# FAILURE line pointing at the saved run log, and a best-effort desktop
# notification via notify-send when available.
#
# State is per HOST, not per checkout — every checkout/worktree shares the
# host's claude binary and subscription quota, so they must share one gate:
#
#   ${XDG_STATE_HOME:-$HOME/.local/state}/delta/e2e-real/last-attempt  gate state
#   ${XDG_STATE_HOME:-$HOME/.local/state}/delta/e2e-real/logs/         run logs
#   ${XDG_STATE_HOME:-$HOME/.local/state}/delta/e2e-real/lock          flock guard
#
# The lock is shared with scripts/e2e-real.sh, so a periodic tick never
# overlaps an in-flight suite run — including a concurrent manual
# `make e2e-real` from any checkout.
#
# Usage: scripts/e2e-real-auto.sh
#   DELTA_CLAUDE_BIN  overrides the claude binary (default: `claude` on PATH).
#   E2E_REAL_CMD      overrides the suite command (testing only; default:
#                     `make e2e-real` in this repository). Run via `bash -c`.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

CLAUDE_BIN="${DELTA_CLAUDE_BIN:-claude}"
STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/delta/e2e-real"
STATE_FILE="$STATE_DIR/last-attempt"
LOG_DIR="$STATE_DIR/logs"
LOCK_FILE="$STATE_DIR/lock"
DEBOUNCE_SECONDS=$((24 * 60 * 60))
KEEP_LOGS=10

log() { printf '\033[1;35m[e2e-real-auto]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[e2e-real-auto]\033[0m %s\n' "$*" >&2; exit 1; }

# --- Preconditions. -----------------------------------------------------------

# A host without claude simply is not a canary host; a periodic driver on such
# a host must tick green and stay quiet.
if ! command -v "$CLAUDE_BIN" >/dev/null 2>&1; then
  log "skipped (claude binary not found: $CLAUDE_BIN — not a canary host)"
  exit 0
fi

# Unlike a missing claude, a missing flock is a broken canary-host setup: the
# overlap guard is what makes a periodic driver safe to install at all.
command -v flock >/dev/null 2>&1 || die "flock not found on PATH (required for the overlap guard)"

current_version="$("$CLAUDE_BIN" --version 2>/dev/null | head -n 1 || true)"
if [ -z "$current_version" ]; then
  log "skipped ('$CLAUDE_BIN --version' produced no output)"
  exit 0
fi

# --- Overlap guard (shared with scripts/e2e-real.sh). --------------------------

mkdir -p "$STATE_DIR" "$LOG_DIR"
exec 9>"$LOCK_FILE"
if ! flock -n 9; then
  log "skipped (another real-claude suite run is in flight; lock: $LOCK_FILE)"
  exit 0
fi

# --- Gate: version change + 24h debounce. --------------------------------------

last_version=""
last_epoch=0
if [ -f "$STATE_FILE" ]; then
  while IFS='=' read -r key value; do
    case "$key" in
      version) last_version="$value" ;;
      epoch) last_epoch="$value" ;;
    esac
  done <"$STATE_FILE"
fi
case "$last_epoch" in
  '' | *[!0-9]*) last_epoch=0 ;;
esac

now="$(date +%s)"

if [ "$current_version" = "$last_version" ]; then
  log "skipped (claude version unchanged since the last attempt: $current_version)"
  exit 0
fi

age=$((now - last_epoch))
if [ "$last_epoch" -gt 0 ] && [ "$age" -lt "$DEBOUNCE_SECONDS" ]; then
  log "skipped (last attempt ${age}s ago, debounce is ${DEBOUNCE_SECONDS}s; '$last_version' -> '$current_version' runs on a later tick)"
  exit 0
fi

# --- Run, recording the attempt regardless of result. --------------------------

attempt_epoch="$now"
attempt_date="$(date -u -d "@$attempt_epoch" +%Y-%m-%dT%H:%M:%SZ)"
log_file="$LOG_DIR/run-$(date -d "@$attempt_epoch" +%Y%m%d-%H%M%S).log"

record_attempt() {
  {
    printf 'version=%s\n' "$current_version"
    printf 'epoch=%s\n' "$attempt_epoch"
    printf 'date=%s\n' "$attempt_date"
    printf 'result=%s\n' "$1"
    printf 'log=%s\n' "$log_file"
  } >"$STATE_FILE"
}

run_suite() {
  # DELTA_E2E_REAL_LOCK_HELD tells e2e-real.sh that this process already holds
  # the shared lock, so it must not try to re-acquire it (that would deadlock
  # the very run the lock was taken for).
  if [ -n "${E2E_REAL_CMD:-}" ]; then
    DELTA_E2E_REAL_LOCK_HELD=1 bash -c "$E2E_REAL_CMD"
  else
    DELTA_E2E_REAL_LOCK_HELD=1 make -C "$REPO_ROOT" e2e-real
  fi
}

# Recorded before the run so a crash or kill mid-suite still counts as an
# attempt (quota was spent); overwritten with the real result afterwards.
record_attempt interrupted

log "claude version changed: '${last_version:-<none>}' -> '$current_version'; running the suite"
log "run log: $log_file"

status=0
run_suite >"$log_file" 2>&1 || status=$?

# Keep the most recent run logs (failures stay inspectable; the state file
# always points at the latest one).
find "$LOG_DIR" -maxdepth 1 -name 'run-*.log' | sort | head -n -"$KEEP_LOGS" \
  | while IFS= read -r old; do rm -f "$old"; done

if [ "$status" -eq 0 ]; then
  record_attempt success
  log "suite passed (claude $current_version)"
  exit 0
fi

record_attempt "failure (exit $status)"
printf '\033[1;31m[e2e-real-auto]\033[0m FAILURE: real-claude canary suite failed (exit %s) on claude %s — likely upstream contract drift. Log: %s. See docs/guides/development.md (drift runbook).\n' \
  "$status" "$current_version" "$log_file" >&2
if command -v notify-send >/dev/null 2>&1; then
  notify-send -u critical "Delta canary FAILED" \
    "real-claude suite failed on claude $current_version — see $log_file" 2>/dev/null || true
fi
exit "$status"
