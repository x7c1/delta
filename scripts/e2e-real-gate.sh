#!/usr/bin/env bash
#
# e2e-real-gate.sh — gated automatic trigger for the real-agent canary suites.
#
# Meant to be invoked by hand or by a periodic driver (the systemd user timer
# under scripts/systemd/, or a cron line — see docs/guides/development/canary.md,
# "Automatic canary trigger"). Each tick it walks every provider below and runs
# that provider's suite only when BOTH hold:
#
#   (a) the installed CLI version differs from the version recorded at that
#       provider's last attempt, and
#   (b) at least 24 hours have passed since that attempt.
#
#   provider  binary (override)              suite
#   claude    claude (DELTA_CLAUDE_BIN)      make e2e-real-claude
#   codex     codex  (DELTA_CODEX_BIN)       make e2e-real-codex
#
# Rationale: both CLIs auto-update frequently (sometimes several times a day)
# and both suites consume real subscription quota — the claude suite a handful
# of turns, the codex canaries one safe turn plus the vendored app-server
# schema-drift check. The gate caps automatic spend at one run per provider per
# day, spends nothing on days without an update, and never misses an update
# (the version comparison catches up on a later tick once the debounce window
# has passed). Codex drift is exactly why it is gated at all: the vendored
# schema silently sat nine minor versions behind the installed CLI until
# someone happened to run the canaries by hand.
#
# The providers are independent: a host with only one of the two CLIs installed
# gates only that one (the other is skipped quietly — it simply is not a canary
# host for that provider), and a failing suite for one provider still leaves the
# other provider's gate evaluated and run.
#
# The debounce is on the ATTEMPT, not on success: a red canary usually means
# real upstream drift, and auto-retrying it hourly would burn quota without
# producing new information. Failures are loud instead: non-zero exit, one
# FAILURE line per failed provider pointing at the saved run log, and a
# best-effort desktop notification (notify-send, else osascript).
#
# State is per HOST, not per checkout — every checkout/worktree shares the
# host's CLIs and subscription quota, so they must share one gate. Under
# ${XDG_STATE_HOME:-$HOME/.local/state}/delta/e2e-real/:
#
#   <provider>/last-attempt  gate state for that provider
#   <provider>/logs/         that provider's run logs
#   lock                     overlap guard for the whole tick
#
# A pre-provider-aware host has its claude state at the root (`last-attempt`);
# the first tick moves it to `claude/last-attempt` so an existing canary host
# does not re-run its suite for no reason.
#
# The lock is shared with scripts/e2e-real-claude.sh, so a periodic tick never
# overlaps an in-flight suite run — including a concurrent manual
# `make e2e-real-claude` from any checkout. It is `flock` where available and an
# atomic lock directory otherwise, so the script also runs on a stock macOS
# host (which ships neither flock nor the GNU-only `date`/`head` flags this
# script therefore avoids). The directory fallback guards tick against tick
# only: without flock, e2e-real-claude.sh deliberately takes no lock (see its
# own comment), so a manual run can overlap a tick on such a host.
#
# Usage: scripts/e2e-real-gate.sh
#   DELTA_CLAUDE_BIN          overrides the claude binary (default: `claude` on PATH).
#   DELTA_CODEX_BIN           overrides the codex binary (default: `codex` on PATH).
#   E2E_REAL_GATE_PROVIDERS   space-separated provider subset (testing only;
#                             default: `claude codex`).
#   E2E_REAL_CMD              overrides the suite command for whichever provider
#                             is running (testing only; default: the provider's
#                             make target in this repository). Run via `bash -c`,
#                             with E2E_REAL_GATE_PROVIDER naming the provider.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

STATE_ROOT="${XDG_STATE_HOME:-$HOME/.local/state}/delta/e2e-real"
LEGACY_STATE_FILE="$STATE_ROOT/last-attempt"
LOCK_FILE="$STATE_ROOT/lock"
LOCK_DIR="$STATE_ROOT/lock.d"
DEBOUNCE_SECONDS=$((24 * 60 * 60))
KEEP_LOGS=10
KNOWN_PROVIDERS="claude codex"
PROVIDERS="${E2E_REAL_GATE_PROVIDERS:-$KNOWN_PROVIDERS}"

log() { printf '\033[1;35m[e2e-real-gate]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[e2e-real-gate]\033[0m %s\n' "$*" >&2; exit 1; }

# Durations in the log are read by a human scanning a terminal or journalctl,
# who wants to know when the next run is due — "43221s ago, debounce is 86400s"
# makes them do the division.
format_duration() {
  duration_hours=$(($1 / 3600))
  duration_minutes=$((($1 % 3600) / 60))
  if [ "$duration_hours" -gt 0 ] && [ "$duration_minutes" -gt 0 ]; then
    printf '%sh%sm' "$duration_hours" "$duration_minutes"
  elif [ "$duration_hours" -gt 0 ]; then
    printf '%sh' "$duration_hours"
  elif [ "$duration_minutes" -gt 0 ]; then
    printf '%sm' "$duration_minutes"
  else
    printf '%ss' "$1"
  fi
}

# --- Provider table. ----------------------------------------------------------

# The lookups below are called from a command substitution, where `die` would
# only kill the subshell, so the provider list is validated once up front
# instead — a typo in E2E_REAL_GATE_PROVIDERS must be an error, not a silent
# "not a canary host" skip.
for provider in $PROVIDERS; do
  case " $KNOWN_PROVIDERS " in
    *" $provider "*) ;;
    *) die "unknown provider: $provider (known: $KNOWN_PROVIDERS)" ;;
  esac
done

provider_bin() {
  case "$1" in
    claude) printf '%s' "${DELTA_CLAUDE_BIN:-claude}" ;;
    codex) printf '%s' "${DELTA_CODEX_BIN:-codex}" ;;
  esac
}

provider_make_target() {
  case "$1" in
    claude) printf 'e2e-real-claude' ;;
    codex) printf 'e2e-real-codex' ;;
  esac
}

# --- Desktop notification (best-effort, never fatal). ---------------------------

# AppleScript string literals only understand backslash escapes, and the body
# carries a version string and a path, so escape before interpolating.
applescript_escape() { printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g'; }

notify() {
  title="$1"
  body="$2"
  if command -v notify-send >/dev/null 2>&1; then
    notify-send -u critical "$title" "$body" 2>/dev/null || true
  elif command -v osascript >/dev/null 2>&1; then
    osascript -e "display notification \"$(applescript_escape "$body")\" with title \"$(applescript_escape "$title")\"" \
      >/dev/null 2>&1 || true
  fi
}

# --- Overlap guard (shared with scripts/e2e-real-claude.sh). --------------------

# flock is the guard on hosts that have it (it is released by the kernel when
# the process dies, and e2e-real-claude.sh takes the same file). Stock macOS
# ships no flock, so fall back to an atomic lock directory: `mkdir` on an
# existing directory fails, which makes the create-or-fail test race-free. The
# owner pid is recorded so a lock left behind by a killed tick can be reclaimed
# instead of wedging the gate forever.
lock_dir_held=""
lock_path="$LOCK_FILE"

claim_lock_dir() {
  mkdir "$LOCK_DIR" 2>/dev/null || return 1
  printf '%s\n' "$$" >"$LOCK_DIR/pid"
  lock_dir_held=1
}

acquire_lock() {
  if command -v flock >/dev/null 2>&1; then
    exec 9>"$LOCK_FILE"
    flock -n 9 || return 1
    return 0
  fi

  lock_path="$LOCK_DIR"
  claim_lock_dir && return 0

  owner="$(cat "$LOCK_DIR/pid" 2>/dev/null || true)"
  case "$owner" in
    '' | *[!0-9]*) owner="" ;;
  esac
  if [ -n "$owner" ] && kill -0 "$owner" 2>/dev/null; then
    return 1
  fi

  log "reclaiming a stale lock directory (owner pid ${owner:-<unknown>} is gone): $LOCK_DIR"
  rm -rf "$LOCK_DIR"
  claim_lock_dir
}

release_lock() {
  if [ -n "$lock_dir_held" ]; then
    rm -rf "$LOCK_DIR"
    lock_dir_held=""
  fi
}
trap release_lock EXIT

mkdir -p "$STATE_ROOT"
if ! acquire_lock; then
  log "skipped (another real-agent suite run is in flight; lock: $lock_path)"
  exit 0
fi

# --- Per-provider gate. ---------------------------------------------------------

overall_status=0
summary=""

add_summary() { summary="$summary  $1: $2"$'\n'; }

# Runs one provider's gate. Never returns non-zero: a provider that fails
# records its failure in $overall_status and lets the loop reach the next one.
gate_provider() {
  provider="$1"
  bin="$(provider_bin "$provider")"

  # A host without this CLI simply is not a canary host for this provider; a
  # periodic driver on such a host must tick green and stay quiet.
  if ! command -v "$bin" >/dev/null 2>&1; then
    log "$provider: skipped ($provider binary not found: $bin — not a canary host)"
    add_summary "$provider" "skipped (binary not found)"
    return 0
  fi

  current_version="$("$bin" --version 2>/dev/null | head -n 1 || true)"
  if [ -z "$current_version" ]; then
    log "$provider: skipped ('$bin --version' produced no output)"
    add_summary "$provider" "skipped (no version output)"
    return 0
  fi

  state_dir="$STATE_ROOT/$provider"
  state_file="$state_dir/last-attempt"
  log_dir="$state_dir/logs"

  # Migration from the single-provider layout: the root state file was always
  # claude's. Moving it keeps an existing canary host from re-running the suite
  # just because the state moved house.
  if [ "$provider" = "claude" ] && [ -f "$LEGACY_STATE_FILE" ] && [ ! -f "$state_file" ]; then
    mkdir -p "$state_dir"
    mv "$LEGACY_STATE_FILE" "$state_file"
    log "$provider: migrated the pre-provider-aware state file to $state_file"
  fi

  mkdir -p "$log_dir"

  last_version=""
  last_epoch=0
  last_result=""
  last_log=""
  if [ -f "$state_file" ]; then
    while IFS='=' read -r key value; do
      case "$key" in
        version) last_version="$value" ;;
        epoch) last_epoch="$value" ;;
        result) last_result="$value" ;;
        log) last_log="$value" ;;
      esac
    done <"$state_file"
  fi
  case "$last_epoch" in
    '' | *[!0-9]*) last_epoch=0 ;;
  esac

  now="$(date +%s)"

  if [ "$current_version" = "$last_version" ]; then
    log "$provider: skipped (version unchanged since the last attempt: $current_version)"
    # A red attempt is not retried until the CLI updates again (the debounce is
    # on the attempt). Repeat that verdict on every later tick: otherwise the
    # only trace of it is the FAILURE line from the tick that produced it, and
    # every tick after that reads as green.
    if [ -n "$last_result" ] && [ "$last_result" != "success" ]; then
      unresolved="$provider: that attempt did not pass ($last_result) and is not retried until $provider updates again"
      if [ -n "$last_log" ]; then
        unresolved="$unresolved; log: $last_log"
      fi
      log "$unresolved"
      add_summary "$provider" "skipped (version unchanged; last attempt: $last_result)"
    else
      add_summary "$provider" "skipped (version unchanged)"
    fi
    return 0
  fi

  age=$((now - last_epoch))
  if [ "$last_epoch" -gt 0 ] && [ "$age" -lt "$DEBOUNCE_SECONDS" ]; then
    log "$provider: skipped (last attempt $(format_duration "$age") ago, debounce is $(format_duration "$DEBOUNCE_SECONDS"); '$last_version' -> '$current_version' runs on a later tick, in about $(format_duration $((DEBOUNCE_SECONDS - age))))"
    add_summary "$provider" "skipped (deferred by the debounce)"
    return 0
  fi

  # --- Run, recording the attempt regardless of result. -------------------------

  # The attempt is happening now, so the timestamps are just the current time
  # in two formats — no epoch-to-date conversion, which only GNU date can do.
  attempt_epoch="$now"
  attempt_date="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  log_file="$log_dir/run-$(date +%Y%m%d-%H%M%S).log"

  record_attempt() {
    {
      printf 'version=%s\n' "$current_version"
      printf 'epoch=%s\n' "$attempt_epoch"
      printf 'date=%s\n' "$attempt_date"
      printf 'result=%s\n' "$1"
      printf 'log=%s\n' "$log_file"
    } >"$state_file"
  }

  run_suite() {
    # DELTA_E2E_REAL_LOCK_HELD tells e2e-real-claude.sh that this process already
    # holds the shared lock, so it must not try to re-acquire it (that would
    # deadlock the very run the lock was taken for). E2E_REAL_GATE_PROVIDER lets
    # an E2E_REAL_CMD stub tell which provider it was invoked for.
    if [ -n "${E2E_REAL_CMD:-}" ]; then
      DELTA_E2E_REAL_LOCK_HELD=1 E2E_REAL_GATE_PROVIDER="$provider" bash -c "$E2E_REAL_CMD"
    else
      DELTA_E2E_REAL_LOCK_HELD=1 E2E_REAL_GATE_PROVIDER="$provider" \
        make -C "$REPO_ROOT" "$(provider_make_target "$provider")"
    fi
  }

  # Recorded before the run so a crash or kill mid-suite still counts as an
  # attempt (quota was spent); overwritten with the real result afterwards.
  record_attempt interrupted

  log "$provider: version changed: '${last_version:-<none>}' -> '$current_version'; running the suite"
  log "$provider: run log: $log_file"

  status=0
  run_suite >"$log_file" 2>&1 || status=$?

  # Keep the most recent run logs (failures stay inspectable; the state file
  # always points at the latest one). Newest first, then drop everything past
  # the keep count — `sort -r | tail -n +N` is the portable spelling.
  find "$log_dir" -maxdepth 1 -name 'run-*.log' | sort -r | tail -n +"$((KEEP_LOGS + 1))" \
    | while IFS= read -r old; do rm -f "$old"; done

  if [ "$status" -eq 0 ]; then
    record_attempt success
    log "$provider: suite passed ($current_version)"
    add_summary "$provider" "ran: success"
    return 0
  fi

  record_attempt "failure (exit $status)"
  add_summary "$provider" "ran: failure (exit $status)"
  overall_status="$status"
  printf '\033[1;31m[e2e-real-gate]\033[0m FAILURE: real-%s canary suite failed (exit %s) on %s — likely upstream contract drift. Log: %s. See docs/guides/development/canary.md (drift runbook).\n' \
    "$provider" "$status" "$current_version" "$log_file" >&2
  notify "Delta canary FAILED" \
    "real-$provider suite failed on $current_version — see $log_file"
  return 0
}

for provider in $PROVIDERS; do
  gate_provider "$provider"
done

log "tick summary:"
printf '%s' "$summary"

exit "$overall_status"
