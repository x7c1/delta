#!/usr/bin/env bash
#
# dev.sh — bring up Delta's local loop.
#
# Wires the whole loop together so you can type in the browser and watch a real
# `claude` TUI (running in tmux) answer, with the response flowing back through
# the JSONL transcript and Claude Code's HTTP hooks:
#
#   browser  ──REST──▶  delta-server  ──send-keys──▶  tmux pane (claude)
#      ▲                     ▲                              │
#      └──── WebSocket ──────┴──── hooks (HTTP) ◀───────────┘
#
# What it does:
#   1. Starts `delta-server` (DELTA_PORT=7878). The server owns the claude
#      session lifecycle: it lazily creates the tmux session running `claude`
#      (and writes its `.claude/settings.json` so the hooks point back at the
#      server) the first time the browser asks for it.
#   2. Starts the frontend dev server against the real backend.
#
# Opening the browser is the only manual step: when the web UI loads it asks the
# server to bring the session up, so tmux is a hidden implementation detail.
#
# Authentication is assumed: the server relies on a cached Claude Code token (or
# CLAUDE_CODE_OAUTH_TOKEN) and does not run interactive OAuth. If `claude` is not
# yet authenticated, run `claude` once on its own (or attach to a spawned pane
# with `tmux attach -t delta-1`) to complete login, then reload the browser.
#
# Usage:
#   scripts/dev.sh [WORKDIR]   # bring the loop up (default WORKDIR: .tmp/session)
#   scripts/dev.sh --down      # tear the loop down (same as scripts/stop.sh)
#   scripts/dev.sh --reset     # tear down, then delete the SQLite database so the
#                              # next start recreates an empty schema (same as
#                              # scripts/reset.sh). Honors DELTA_DB_PATH.
#   scripts/dev.sh --help

set -euo pipefail

# Resolve repo paths regardless of where the script is invoked from.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BACKEND_DIR="$REPO_ROOT/backend"
FRONTEND_DIR="$REPO_ROOT/frontend"

# The server mints a unique tmux session per spawn, all named `delta-<n>`. The
# script never names a session itself; it only uses this prefix to reap them on
# teardown.
TMUX_SESSION_PREFIX="delta-"
DELTA_PORT="7878"
FRONTEND_PORT="5173"
DEFAULT_WORKDIR="$REPO_ROOT/.tmp/session"

# The SQLite database delta-server opens. The server defaults to `delta.db`
# relative to its cwd (the backend dir); honor DELTA_DB_PATH if the developer
# overrode it. `--reset` deletes this so the next start recreates empty schema.
DELTA_DB="${DELTA_DB_PATH:-$BACKEND_DIR/delta.db}"

# delta-server and the frontend dev server each log to a per-run timestamped file
# so a new run never clobbers a previous run's log. A stable `*.log` symlink
# always points at the most recent run for convenience.
LOG_DIR="$REPO_ROOT/.tmp"
STAMP="$(date +%Y%m%d-%H%M%S)"
SERVER_LOG="$LOG_DIR/delta-server-$STAMP.log"
SERVER_LOG_LATEST="$LOG_DIR/delta-server.log"
FRONTEND_LOG="$LOG_DIR/delta-frontend-$STAMP.log"
FRONTEND_LOG_LATEST="$LOG_DIR/delta-frontend.log"

log()  { printf '\033[1;36m[delta]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[delta]\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m[delta]\033[0m %s\n' "$*" >&2; exit 1; }

usage() {
  # Print the leading comment block (everything up to the first blank,
  # non-comment line), stripping the leading "# ".
  awk 'NR>1 { if ($0 !~ /^#/) exit; sub(/^# ?/, ""); print }' "${BASH_SOURCE[0]}"
}

# Tear everything down: stop the server, the frontend dev server, and the tmux
# session the server created for `claude`.
down() {
  log "Stopping delta-server (port $DELTA_PORT) ..."
  # delta-server binds 127.0.0.1:$DELTA_PORT; match the listener and kill it.
  if command -v pkill >/dev/null 2>&1; then
    pkill -f "delta-server" 2>/dev/null || true
  fi
  # Belt-and-suspenders: also free the port in case the listener's argv did not
  # match (e.g. it is still the `cargo run` parent that has not exec'd the
  # binary yet).
  kill_port "$DELTA_PORT"

  # Stop the frontend dev server. `pnpm --filter @delta/web dev` execs `vite`,
  # whose own argv contains neither "@delta/web" nor "dev", so a name-based
  # pkill misses the actual port holder and leaves it orphaned. Kill by port —
  # whatever is listening on $FRONTEND_PORT is the dev server — which also
  # catches vite's child esbuild service.
  log "Stopping frontend dev server (port $FRONTEND_PORT) ..."
  if command -v pkill >/dev/null 2>&1; then
    # Best-effort: also reap the pnpm parent so it does not respawn the child.
    pkill -f "@delta/web.*dev" 2>/dev/null || true
  fi
  kill_port "$FRONTEND_PORT"

  # The server mints one tmux session per spawn (`delta-<n>`), so kill every
  # session whose name starts with the prefix rather than a single fixed name.
  if command -v tmux >/dev/null 2>&1; then
    tmux list-sessions -F '#{session_name}' 2>/dev/null \
      | grep -E "^${TMUX_SESSION_PREFIX}" \
      | while read -r session; do
          log "Killing tmux session '$session' ..."
          tmux kill-session -t "$session" 2>/dev/null || true
        done
  fi
  log "Down."
}

# Tear everything down, then delete the SQLite database (and its WAL/SHM
# sidecars) so the next start recreates an empty schema. The server applies the
# schema on open via `CREATE TABLE/INDEX IF NOT EXISTS`, so a fresh file is all
# it takes. The server must be stopped first (down) so it is not still writing.
reset() {
  down
  log "Deleting SQLite database: $DELTA_DB ..."
  rm -f "$DELTA_DB" "$DELTA_DB-wal" "$DELTA_DB-shm"
  log "Database reset. The next 'scripts/dev.sh' will recreate an empty schema."
}

require() {
  command -v "$1" >/dev/null 2>&1 || die "'$1' not found on PATH. $2"
}

# Whether something is already listening on 127.0.0.1:$1. Best-effort: tries the
# common tools and reports "not listening" if none are available.
port_in_use() {
  local port="$1"
  if command -v lsof >/dev/null 2>&1; then
    lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1 && return 0
  elif command -v ss >/dev/null 2>&1; then
    ss -ltn 2>/dev/null | grep -qE "[:.]$port[[:space:]]" && return 0
  elif command -v netstat >/dev/null 2>&1; then
    netstat -ltn 2>/dev/null | grep -qE "[:.]$port[[:space:]]" && return 0
  fi
  return 1
}

# Kill whatever is listening on 127.0.0.1:$1. Used by teardown so a process whose
# argv does not match a name-based pkill (notably vite, which holds the frontend
# port) is still stopped reliably. Best-effort: needs lsof or fuser; if neither
# is present it logs a hint and leaves the port-based kill to the name match.
kill_port() {
  local port="$1"
  if command -v lsof >/dev/null 2>&1; then
    local pids
    pids="$(lsof -t -nP -iTCP:"$port" -sTCP:LISTEN 2>/dev/null || true)"
    if [ -n "$pids" ]; then
      # shellcheck disable=SC2086
      kill $pids 2>/dev/null || true
    fi
  elif command -v fuser >/dev/null 2>&1; then
    fuser -k "$port/tcp" >/dev/null 2>&1 || true
  else
    warn "Cannot free port $port (no lsof/fuser). If something is still listening, kill it manually."
  fi
}

up() {
  local workdir="${1:-$DEFAULT_WORKDIR}"

  # --- Preflight: every moving part must exist before we touch anything. ---
  require tmux  "Install tmux to host the claude session."
  require claude "Install Claude Code and authenticate it first (run 'claude' once)."
  require cargo "Install the Rust toolchain (https://rustup.rs)."
  require pnpm  "Enable corepack ('corepack enable') so pnpm is on PATH."

  # The server mints a unique tmux session per spawn (`delta-<n>`), so there is
  # no fixed name to collide with up front. Any leftover `delta-*` sessions from
  # a previous run are reaped by teardown ('scripts/dev.sh --down').

  # A running server already owns the port; starting a second one would fail
  # with "Address already in use" partway through. Abort cleanly up front and
  # point at --down rather than clobbering the running server's state.
  if port_in_use "$DELTA_PORT"; then
    die "A server is already listening on 127.0.0.1:$DELTA_PORT. Run 'scripts/dev.sh --down' first (or stop whatever owns the port)."
  fi
  if port_in_use "$FRONTEND_PORT"; then
    die "Something is already listening on 127.0.0.1:$FRONTEND_PORT (the frontend dev server). Run 'scripts/dev.sh --down' first."
  fi

  # The base working directory for spawns; resolve it to an absolute path. The
  # server creates a per-spawn `<base>/<token>` subdirectory under it on demand
  # and provisions that subdirectory's .claude/settings.json.
  workdir="$(mkdir -p "$workdir" && cd "$workdir" && pwd)"
  log "Session workdir base: $workdir (the server provisions <base>/<token>/.claude/settings.json per spawn)"

  mkdir -p "$LOG_DIR"

  # --- 1. delta-server. It owns the claude session lifecycle. ---
  ln -sf "$(basename "$SERVER_LOG")" "$SERVER_LOG_LATEST"
  log "Starting delta-server (DELTA_PORT=$DELTA_PORT) ..."
  log "Server log: $SERVER_LOG (latest -> $SERVER_LOG_LATEST)"
  (
    cd "$BACKEND_DIR"
    DELTA_PORT="$DELTA_PORT" \
      DELTA_SESSION_WORKDIR="$workdir" \
      cargo run -p delta-server >"$SERVER_LOG" 2>&1
  ) &

  # --- 2. Frontend dev server against the real backend. ---
  ln -sf "$(basename "$FRONTEND_LOG")" "$FRONTEND_LOG_LATEST"
  log "Building frontend workspace libraries and starting the dev server (port $FRONTEND_PORT) ..."
  log "Frontend log: $FRONTEND_LOG (latest -> $FRONTEND_LOG_LATEST)"
  (
    cd "$FRONTEND_DIR"
    pnpm install >"$FRONTEND_LOG" 2>&1
    pnpm -r build >>"$FRONTEND_LOG" 2>&1
    pnpm --filter @delta/web dev >>"$FRONTEND_LOG" 2>&1
  ) &

  cat <<EOF

────────────────────────────────────────────────────────────────────────────
Delta is coming up.

  • delta-server  → http://127.0.0.1:$DELTA_PORT  (owns the claude session)
  • frontend      → http://localhost:$FRONTEND_PORT  (building libs first; give it a moment)

The only manual step: open the browser.

  Open:  http://localhost:$FRONTEND_PORT

When the UI loads it asks the server to start the claude session, so there is
nothing else to launch. Authentication is assumed (cached token /
CLAUDE_CODE_OAUTH_TOKEN). If claude is not yet authenticated, run 'claude' once
on its own — or attach to a spawned pane — to complete login, then reload:

       tmux attach -t ${TMUX_SESSION_PREFIX}1      # detach with Ctrl-b then d

When done, shut everything down (server + frontend + tmux sessions):

       scripts/dev.sh --down      # or: scripts/stop.sh

────────────────────────────────────────────────────────────────────────────
EOF
}

main() {
  case "${1:-}" in
    --down|down)   down ;;
    --reset|reset) reset ;;
    -h|--help|help) usage ;;
    *)             up "${1:-}" ;;
  esac
}

main "$@"
