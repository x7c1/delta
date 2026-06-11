#!/usr/bin/env bash
#
# e2e-fake.sh — run the fake-mode Playwright suite against a real backend.
#
# Boots a real `delta-server` (temp database, dedicated port, per-run tmux
# socket) whose spawned "claude" is the scripted `fake-claude` binary, then
# runs the Playwright suite in frontend/packages/apps/web/e2e-fake against it.
# Everything between the browser and the scripted model is real: REST, the
# WebSocket event channel, the PTY bridge, tmux panes, hooks, and the JSONL
# transcript tail.
#
# Per-run isolation:
#   - the SQLite database, spawn workdirs, and fake transcripts live in a
#     fresh temp directory, deleted on exit;
#   - tmux runs on a unique per-run socket (delta-e2e-<pid>), killed on exit,
#     so parallel or leftover runs never collide;
#   - dedicated ports (backend 7899, web 5198 by default) so a live
#     `make dev` (7878/5173) or the mock e2e suite (5199) is never touched.
#
# Scenario routing: the server's spawn command is fixed, so the wrapper script
# this run generates pins FAKE_CLAUDE_SCENARIO_DIR at the suite's scenarios/
# directory; each spec picks its scenario via the first word of the first
# prompt it sends (see fake-claude's scenario module).
#
# Usage: scripts/e2e-fake.sh
#   E2E_FAKE_BACKEND_PORT / E2E_FAKE_PORT override the ports.
#
# Prerequisites: tmux, the Rust toolchain, pnpm (workspace installed and
# libraries built: `pnpm install && pnpm -r build` in frontend/), and the
# Playwright chromium browser.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BACKEND_DIR="$REPO_ROOT/backend"
FRONTEND_DIR="$REPO_ROOT/frontend"
SCENARIO_DIR="$FRONTEND_DIR/packages/apps/web/e2e-fake/scenarios"

BACKEND_PORT="${E2E_FAKE_BACKEND_PORT:-7899}"
WEB_PORT="${E2E_FAKE_PORT:-5198}"
TMUX_SOCKET="delta-e2e-$$"

log() { printf '\033[1;36m[e2e-fake]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[e2e-fake]\033[0m %s\n' "$*" >&2; exit 1; }

command -v tmux >/dev/null 2>&1 || die "tmux not found on PATH"
command -v cargo >/dev/null 2>&1 || die "cargo not found on PATH"
command -v pnpm >/dev/null 2>&1 || die "pnpm not found on PATH"

# --- Build both binaries up front so the server start below is instant. -----
log "Building delta-server and fake-claude ..."
(cd "$BACKEND_DIR" && cargo build -p delta-server -p fake-claude)

# --- Per-run state, torn down on exit. ---------------------------------------
RUN_DIR="$(mktemp -d "${TMPDIR:-/tmp}/delta-e2e-fake.XXXXXX")"
SERVER_PID=""

teardown() {
  if [ -n "$SERVER_PID" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  # Kill the whole per-run tmux server: every pane this run spawned dies with
  # it, and other sockets (a developer's `delta`, another run) are untouched.
  tmux -L "$TMUX_SOCKET" kill-server 2>/dev/null || true
  rm -rf "$RUN_DIR"
}
trap teardown EXIT

mkdir -p "$RUN_DIR/workdir" "$RUN_DIR/transcripts"

# The spawn command line is fixed, so per-run configuration reaches the fake
# through this wrapper: it pins the scenario directory and the transcript
# directory, then forwards the CLI args delta-server passes.
WRAPPER="$RUN_DIR/claude-bin.sh"
cat > "$WRAPPER" <<EOF
#!/bin/sh
FAKE_CLAUDE_SCENARIO_DIR='$SCENARIO_DIR' \\
FAKE_CLAUDE_TRANSCRIPT_DIR='$RUN_DIR/transcripts' \\
exec '$BACKEND_DIR/target/debug/fake-claude' "\$@"
EOF
chmod +x "$WRAPPER"

# --- Start the backend. -------------------------------------------------------
log "Starting delta-server on 127.0.0.1:$BACKEND_PORT (tmux socket: $TMUX_SOCKET) ..."
log "Server log: $RUN_DIR/server.log"
DELTA_PORT="$BACKEND_PORT" \
  DELTA_DB_PATH="$RUN_DIR/delta.db" \
  DELTA_SESSION_WORKDIR="$RUN_DIR/workdir" \
  DELTA_TMUX_SOCKET="$TMUX_SOCKET" \
  DELTA_CLAUDE_BIN="$WRAPPER" \
  DELTA_LAUNCH_DEADLINE_MS=3000 \
  "$BACKEND_DIR/target/debug/delta-server" >"$RUN_DIR/server.log" 2>&1 &
SERVER_PID=$!

# Wait for the health endpoint instead of sleeping a fixed time.
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

# --- Run the suite (Playwright owns the Vite dev server). ---------------------
log "Running the fake-mode Playwright suite (web port $WEB_PORT) ..."
status=0
(
  cd "$FRONTEND_DIR"
  E2E_FAKE_BACKEND_PORT="$BACKEND_PORT" E2E_FAKE_PORT="$WEB_PORT" \
    pnpm --filter @delta/web e2e:fake
) || status=$?
log "Suite finished (exit $status)."
exit "$status"
