#!/usr/bin/env bash
#
# dev.sh — bring up Delta's local walking skeleton.
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
#   1. Prepares a working directory for the `claude` session and drops a
#      `.claude/settings.json` there so the session's hooks point at the local
#      server.
#   2. Starts a tmux session named `delta` with `claude` in pane `delta:0.0`.
#   3. Starts `delta-server` (DELTA_TMUX_PANE=delta:0.0, DELTA_PORT=7878).
#   4. Prints the exact command to start the frontend dev server against the
#      real backend, plus how to attach to the TUI and how to shut everything
#      down.
#
# This is the minimal wiring: permission prompts are answered in the TUI
# (`tmux attach -t delta`); robustness and edge-cases come later.
#
# Usage:
#   scripts/dev.sh [WORKDIR]   # bring the loop up (default WORKDIR: .tmp/session)
#   scripts/dev.sh --down      # tear the loop down (same as scripts/stop.sh)
#   scripts/dev.sh --help

set -euo pipefail

# Resolve repo paths regardless of where the script is invoked from.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BACKEND_DIR="$REPO_ROOT/backend"
FRONTEND_DIR="$REPO_ROOT/frontend"
SETTINGS_TEMPLATE="$SCRIPT_DIR/claude-settings.json"

TMUX_SESSION="delta"
TMUX_PANE="delta:0.0"
DELTA_PORT="7878"
DEFAULT_WORKDIR="$REPO_ROOT/.tmp/session"

# delta-server logs to this file when started in the background.
SERVER_LOG="$REPO_ROOT/.tmp/delta-server.log"

log()  { printf '\033[1;36m[delta]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[delta]\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m[delta]\033[0m %s\n' "$*" >&2; exit 1; }

usage() {
  # Print the leading comment block (everything up to the first blank,
  # non-comment line), stripping the leading "# ".
  awk 'NR>1 { if ($0 !~ /^#/) exit; sub(/^# ?/, ""); print }' "${BASH_SOURCE[0]}"
}

# Tear everything down: stop the server, then kill the tmux session.
down() {
  log "Stopping delta-server (port $DELTA_PORT) ..."
  # delta-server binds 127.0.0.1:$DELTA_PORT; match the listener and kill it.
  if command -v pkill >/dev/null 2>&1; then
    pkill -f "delta-server" 2>/dev/null || true
  fi

  if command -v tmux >/dev/null 2>&1 && tmux has-session -t "$TMUX_SESSION" 2>/dev/null; then
    log "Killing tmux session '$TMUX_SESSION' ..."
    tmux kill-session -t "$TMUX_SESSION" 2>/dev/null || true
  fi
  log "Down."
}

require() {
  command -v "$1" >/dev/null 2>&1 || die "'$1' not found on PATH. $2"
}

up() {
  local workdir="${1:-$DEFAULT_WORKDIR}"

  # --- Preflight: every moving part must exist before we touch anything. ---
  require tmux  "Install tmux to host the claude session."
  require claude "Install Claude Code and authenticate it first (run 'claude' once)."
  require cargo "Install the Rust toolchain (https://rustup.rs)."
  require pnpm  "Enable corepack ('corepack enable') so pnpm is on PATH."
  [ -f "$SETTINGS_TEMPLATE" ] || die "Settings template not found: $SETTINGS_TEMPLATE"

  if tmux has-session -t "$TMUX_SESSION" 2>/dev/null; then
    die "A tmux session named '$TMUX_SESSION' already exists. Run 'scripts/dev.sh --down' (or 'tmux kill-session -t $TMUX_SESSION') first."
  fi

  # --- 1. Working directory + per-session hook settings. ---
  workdir="$(mkdir -p "$workdir" && cd "$workdir" && pwd)"
  mkdir -p "$workdir/.claude"
  cp "$SETTINGS_TEMPLATE" "$workdir/.claude/settings.json"
  log "Session workdir: $workdir"
  log "Hooks installed:  $workdir/.claude/settings.json (-> 127.0.0.1:$DELTA_PORT)"

  # --- 2. tmux session running claude. ---
  log "Starting tmux session '$TMUX_SESSION' with claude in pane '$TMUX_PANE' ..."
  tmux new-session -d -s "$TMUX_SESSION" -c "$workdir" claude

  # --- 3. delta-server, pointed at the tmux pane. ---
  mkdir -p "$(dirname "$SERVER_LOG")"
  log "Starting delta-server (DELTA_PORT=$DELTA_PORT, DELTA_TMUX_PANE=$TMUX_PANE) ..."
  log "Server log: $SERVER_LOG"
  (
    cd "$BACKEND_DIR"
    DELTA_PORT="$DELTA_PORT" DELTA_TMUX_PANE="$TMUX_PANE" \
      cargo run -p delta-server >"$SERVER_LOG" 2>&1
  ) &

  cat <<EOF

────────────────────────────────────────────────────────────────────────────
Delta walking skeleton is coming up.

Next steps:

  1. Start the frontend dev server against the REAL backend (separate terminal):

       cd "$FRONTEND_DIR"
       pnpm install            # first run only
       pnpm -r build           # build workspace libs first
       pnpm --filter @delta/web dev

     Then open:  http://localhost:5173

  2. Attach to the claude TUI to complete first-run OAuth login and to answer
     permission prompts as they appear:

       tmux attach -t $TMUX_SESSION      # detach with Ctrl-b then d

  3. Type a message in the browser. It is sent into the tmux pane via
     send-keys; claude's reply flows back through the transcript + hooks and
     appears in the browser.

When done, shut everything down:

       scripts/dev.sh --down      # or: scripts/stop.sh

────────────────────────────────────────────────────────────────────────────
EOF
}

main() {
  case "${1:-}" in
    --down|down)   down ;;
    -h|--help|help) usage ;;
    *)             up "${1:-}" ;;
  esac
}

main "$@"
