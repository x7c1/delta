#!/usr/bin/env bash
#
# e2e-real.sh — run the real-claude canary suite against the real `claude` CLI.
#
# This lane is contract monitoring, not feature testing: it checks that the
# implicit upstream contract Delta depends on — hook events and their payload
# fields, the JSONL transcript shapes, the interrupt marker, the
# `queued_command` attachment, `isMeta` flagging, the permission-decision
# envelope — still holds against the real `claude` binary. The fake-claude
# lane (`make e2e-fake`) re-enacts that contract deterministically; this lane
# is what keeps the re-enactment honest. See docs/guides/development/canary.md
# ("Real-claude canaries") for when to run it and what to do
# when it breaks.
#
# Two layers, cheapest first:
#
#   1. Rust canaries (backend/crates/apps/delta-server/tests/real_claude_canary.rs):
#      drive `claude` in tmux directly with Delta's exact spawn shape
#      (rendered --settings, --session-id, positional prompt) and capture the
#      raw hook POSTs and the raw transcript JSONL. No browser, no server —
#      the contract is asserted at the wire.
#   2. One Playwright smoke spec (frontend/packages/apps/web/e2e-real/):
#      browser → real delta-server → tmux → real claude → transcript → browser,
#      proving the full loop closes against the real binary.
#
# Quota: every canary uses the smallest possible prompt and the whole suite is
# a handful of real turns. It still consumes the authenticated user's Claude
# subscription quota, so it is run locally on demand — never in CI (GitHub
# runners have no claude auth).
#
# Per-run isolation mirrors e2e-fake.sh: temp database/workdirs, per-run tmux
# sockets, dedicated ports (backend 7897, web 5197 by default) so a live
# `make dev` (7878/5173), the mock suite (5199), and the fake suite
# (7899/5198) are never touched.
#
# Usage: scripts/e2e-real.sh
#   E2E_REAL_BACKEND_PORT / E2E_REAL_PORT override the ports.
#   DELTA_CLAUDE_BIN overrides the binary (default: `claude` on PATH).
#
# Prerequisites: tmux, the Rust toolchain, pnpm (workspace installed and
# libraries built: `pnpm install && pnpm -r build` in frontend/), the
# Playwright chromium browser, and an authenticated `claude` CLI.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BACKEND_DIR="$REPO_ROOT/backend"
FRONTEND_DIR="$REPO_ROOT/frontend"

BACKEND_PORT="${E2E_REAL_BACKEND_PORT:-7897}"
WEB_PORT="${E2E_REAL_PORT:-5197}"
CLAUDE_BIN="${DELTA_CLAUDE_BIN:-claude}"
TMUX_SOCKET="delta-e2e-real-$$"

log() { printf '\033[1;35m[e2e-real]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[e2e-real]\033[0m %s\n' "$*" >&2; exit 1; }

command -v tmux >/dev/null 2>&1 || die "tmux not found on PATH"
command -v cargo >/dev/null 2>&1 || die "cargo not found on PATH"
command -v pnpm >/dev/null 2>&1 || die "pnpm not found on PATH"
command -v "$CLAUDE_BIN" >/dev/null 2>&1 || die "claude binary not found: $CLAUDE_BIN"

# Per-host overlap guard, shared with scripts/e2e-real-auto.sh: at most one
# real-claude suite per host at a time, across all checkouts/worktrees (they
# share the host's claude, quota, and ~/.claude state). The auto wrapper
# already holds the lock when it invokes this script and signals that with
# DELTA_E2E_REAL_LOCK_HELD; re-acquiring here would deadlock its own run.
# Hosts without flock keep the old unguarded manual behavior.
if [ "${DELTA_E2E_REAL_LOCK_HELD:-}" != "1" ] && command -v flock >/dev/null 2>&1; then
  LOCK_FILE="${XDG_STATE_HOME:-$HOME/.local/state}/delta/e2e-real/lock"
  mkdir -p "$(dirname "$LOCK_FILE")"
  exec 9>"$LOCK_FILE"
  flock -n 9 || die "another real-claude suite run is in flight (lock: $LOCK_FILE)"
fi

log "This suite drives the REAL claude CLI: it consumes subscription quota."

# --- Layer 1: Rust contract canaries (no server, no browser). -----------------
log "Running the Rust contract canaries ..."
(
  cd "$BACKEND_DIR"
  DELTA_CLAUDE_BIN="$CLAUDE_BIN" cargo test -p delta-server --test real_claude_canary \
    -- --ignored --test-threads=1 --nocapture
)

# --- Layer 2: full-loop browser smoke against a real backend + real claude. ---
log "Building delta-server ..."
(cd "$BACKEND_DIR" && cargo build -p delta-server)

RUN_DIR="$(mktemp -d "${TMPDIR:-/tmp}/delta-e2e-real.XXXXXX")"
# The session's working directory lives INSIDE the repository (not under
# /tmp): a host that develops Delta has already trusted this repository, so
# the real claude never raises a first-run trust prompt for a directory under
# it. The smoke spec navigates the browser's workdir picker to it one segment
# at a time, which imposes two constraints the picker cannot work around: the
# path must be under $HOME, and no segment may start with a dot (the picker
# hides dot-directories). A linked git worktree typically lives under a
# dot-directory (e.g. .tmp/worktrees/...), so the workdir is anchored at the
# MAIN checkout's root — `--git-common-dir` resolves to the main `.git` from
# any worktree — which shares the main checkout's claude trust.
MAIN_REPO_GIT_DIR="$(git -C "$REPO_ROOT" rev-parse --path-format=absolute --git-common-dir)"
WORKDIR="$(dirname "$MAIN_REPO_GIT_DIR")/backend/target/e2e-real/$$"
case "$WORKDIR" in
  "$HOME"/*) ;;
  *) die "smoke workdir must live under \$HOME for the picker: $WORKDIR" ;;
esac
case "${WORKDIR#"$HOME"/}" in
  .*|*/.*) die "smoke workdir path contains a dot segment the picker cannot enter: $WORKDIR" ;;
esac
SERVER_PID=""

teardown() {
  if [ -n "$SERVER_PID" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  # Kill the whole per-run tmux server: the real claude pane this run spawned
  # dies with it; other sockets (a developer's `delta`, the fake lane) are
  # untouched.
  tmux -L "$TMUX_SOCKET" kill-server 2>/dev/null || true
  rm -rf "$RUN_DIR" "$WORKDIR"
  rm -f "${TMPDIR:-/tmp}/delta-tmux-$TMUX_SOCKET.conf"
  # Best-effort removal of the transcript claude wrote for this run's
  # session under ~/.claude/projects (the project directory is the munged
  # working-directory path). The killed claude flushes its transcript once
  # more while shutting down, which can resurrect the file after a single
  # removal — give it a beat first.
  sleep 1
  munged="${WORKDIR//\//-}"
  munged="${munged//./-}"
  rm -rf "$HOME/.claude/projects/$munged"
}
trap teardown EXIT

mkdir -p "$WORKDIR"

log "Starting delta-server on 127.0.0.1:$BACKEND_PORT (tmux socket: $TMUX_SOCKET) ..."
log "Server log: $RUN_DIR/server.log"
# The nested-session markers are stripped (env -u) so the claude this server
# spawns never inherits them: a claude that believes it is a child of another
# Claude Code session does not persist its transcript JSONL, which silently
# breaks the whole loop. Delta in production is launched from a normal shell
# where these are unset; this only matters when the suite itself is driven
# from inside a Claude Code session.
DELTA_PORT="$BACKEND_PORT" \
  DELTA_DB_PATH="$RUN_DIR/delta.db" \
  DELTA_SESSION_WORKDIR="$WORKDIR" \
  DELTA_TMUX_SOCKET="$TMUX_SOCKET" \
  DELTA_CLAUDE_BIN="$CLAUDE_BIN" \
  env -u CLAUDECODE -u CLAUDE_CODE_CHILD_SESSION -u CLAUDE_CODE_SESSION_ID \
      -u CLAUDE_CODE_ENTRYPOINT -u CLAUDE_CODE_EXECPATH -u CLAUDE_EFFORT -u AI_AGENT \
  "$BACKEND_DIR/target/debug/delta-server" >"$RUN_DIR/server.log" 2>&1 &
SERVER_PID=$!

for _ in $(seq 1 100); do
  if curl -sf "http://127.0.0.1:$BACKEND_PORT/health" >/dev/null 2>&1; then
    break
  fi
  kill -0 "$SERVER_PID" 2>/dev/null || {
    cat "$RUN_DIR/server.log" >&2
    die "delta-server exited during startup"
  }
  sleep 0.1
done
curl -sf "http://127.0.0.1:$BACKEND_PORT/health" >/dev/null 2>&1 \
  || die "delta-server did not become healthy on port $BACKEND_PORT"

log "Running the real-mode Playwright smoke (web port $WEB_PORT) ..."
status=0
(
  cd "$FRONTEND_DIR"
  E2E_REAL_BACKEND_PORT="$BACKEND_PORT" E2E_REAL_PORT="$WEB_PORT" \
    E2E_REAL_WORKDIR="$WORKDIR" \
    pnpm --filter @delta/web e2e:real
) || status=$?
log "Suite finished (exit $status)."
exit "$status"
