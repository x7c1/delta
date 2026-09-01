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
#      session lifecycle: spawning a session creates a tmux session running
#      `claude` (and writes its `.claude/settings.json` so the hooks point back
#      at the server). No session is spawned on startup or on page load.
#   2. Starts the frontend dev server against the real backend.
#
# Opening the browser is the only manual step. On load the UI shows the session
# list (empty on a fresh database) — it does not auto-start anything. The first
# Send from the composer (or a New action) spawns a fresh `claude` session;
# Sending to a closed session resumes it. tmux is a hidden implementation detail.
#
# Authentication is assumed: the server relies on a cached Claude Code token (or
# CLAUDE_CODE_OAUTH_TOKEN) and does not run interactive OAuth. If `claude` is not
# yet authenticated, run `claude` once on its own (or attach to a spawned pane
# with `tmux -L delta attach -t delta-1`) to complete login, then reload the browser.
#
# Usage:
#   scripts/dev.sh [WORKDIR]   # bring the loop up (default WORKDIR: .tmp/session)
#   scripts/dev.sh --down      # tear the loop down (same as scripts/stop.sh)
#   scripts/dev.sh --reset     # tear down, then delete the SQLite database — and
#                              # the pre-migration snapshots taken from it — so the
#                              # next start recreates an empty schema (same as
#                              # scripts/reset.sh). Honors DELTA_DB_PATH.
#   scripts/dev.sh --help

set -euo pipefail

# Resolve repo paths regardless of where the script is invoked from.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BACKEND_DIR="$REPO_ROOT/backend"
FRONTEND_DIR="$REPO_ROOT/frontend"

# Delta runs its sessions on a dedicated tmux server (socket `delta`), separate
# from the user's default tmux server. The server mints a unique session per
# spawn (`delta-<n>`) on this socket; teardown just kills the whole socket.
DELTA_TMUX_SOCKET="${DELTA_TMUX_SOCKET:-delta}"
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

  # All Delta sessions live on their own tmux server (socket `$DELTA_TMUX_SOCKET`),
  # so killing that server tears down every spawned session at once without
  # touching the user's default tmux server.
  if command -v tmux >/dev/null 2>&1; then
    if tmux -L "$DELTA_TMUX_SOCKET" has-session 2>/dev/null; then
      log "Killing Delta's tmux server (socket '$DELTA_TMUX_SOCKET') ..."
    fi
    tmux -L "$DELTA_TMUX_SOCKET" kill-server 2>/dev/null || true
  fi
  log "Down."
}

# Tear everything down, then delete the SQLite database (its WAL/SHM sidecars,
# and the `.bak-v*` pre-migration snapshots the ladder took from it) so the next
# start recreates an empty schema. The server builds the schema on open by
# replaying its migration ladder, so a fresh file is all it takes.
#
# The snapshots have to go with the database that produced them. The ladder skips
# taking a backup when `<db>.bak-v<source version>` already exists — that file is
# assumed to be a snapshot of *this* database, from a migration that failed and
# rolled back. A reset starts a new database at the same path, so a snapshot left
# behind by the old one would silently suppress the backup for a later destructive
# migration, and the surviving file would be a snapshot of data that no longer
# exists. Deleting them here is deliberate and is the only place they are removed.
#
# The server must be stopped first (down) so it is not still writing.
reset() {
  down
  log "Deleting SQLite database: $DELTA_DB (with its WAL/SHM sidecars and .bak-v* snapshots) ..."
  # An unmatched `.bak-v*` glob stays literal and `rm -f` ignores it.
  rm -f "$DELTA_DB" "$DELTA_DB-wal" "$DELTA_DB-shm" "$DELTA_DB".bak-v*
  log "Database reset. The next 'scripts/dev.sh' will recreate an empty schema."
}

require() {
  command -v "$1" >/dev/null 2>&1 || die "'$1' not found on PATH. $2"
}

# Mint a random per-run bearer token (64 hex chars). `openssl` is preferred; the
# `/dev/urandom` fallback keeps this portable across macOS and Linux where
# openssl is not guaranteed. `dd`+`od` consume their whole input, so no SIGPIPE
# trips `set -o pipefail` the way a `head -c` on a pipe would.
mint_auth_token() {
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -hex 32
  else
    dd if=/dev/urandom bs=32 count=1 2>/dev/null | od -An -tx1 | tr -d ' \n'
  fi
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

# One in-place status line for wait_until_listening on interactive terminals:
#
#   [delta] Waiting for delta-server on port 7878 (12s) — Compiling delta-usecase v0.3.0
#
# The trailing part is the launcher's newest log line (ANSI codes, leading
# indentation, and carriage returns stripped), which is what names the work the
# wait is actually blocked on — cargo compiling a crate, pnpm installing, the
# workspace libraries building. Truncated to the terminal width because a
# wrapped line breaks the \r redraw. Args: $1=label $2=port $3=waited-secs
# $4=log-path
draw_wait_status() {
  local label="$1" port="$2" waited="$3" log="$4"
  local tail_line cols max body
  tail_line="$(tail -n 1 "$log" 2>/dev/null | sed -e $'s/\x1B\\[[0-9;]*m//g' -e 's/^[[:space:]]*//' | tr -d '\r')"
  body="Waiting for $label on port $port (${waited}s)"
  if [ -n "$tail_line" ]; then
    body="$body — $tail_line"
  fi
  cols="$(tput cols 2>/dev/null || echo 120)"
  max=$((cols - 8)) # visible width of the "[delta] " prefix
  if [ "$max" -lt 20 ]; then max=20; fi
  if [ "${#body}" -gt "$max" ]; then body="${body:0:max}"; fi
  printf '\r\033[2K\033[1;36m[delta]\033[0m %s' "$body"
}

# Block until 127.0.0.1:$port is accepting connections, returning 0 as soon as it
# is. This is what makes `dev.sh` return only once the loop is actually reachable
# — historically it launched the server and dev server in the background and
# exited immediately, so "I ran make dev but the browser won't open" was common
# (the frontend's install+build had not finished binding the port yet).
#
# On an interactive terminal the wait redraws a single status line showing the
# elapsed seconds and the launcher's newest log line (see draw_wait_status), so
# it is visible what the wait is blocked on and that it is progressing. When
# stdout is not a tty (piped, CI) it keeps the classic dot-per-second stream,
# since an in-place \r redraw would garble line-oriented output.
#
# Fails fast (returns non-zero) if the launching process dies before the port
# comes up, or if $timeout seconds elapse — in both cases the caller surfaces the
# log. Args: $1=port $2=label $3=launcher-pid $4=timeout-secs $5=log-path
wait_until_listening() {
  local port="$1" label="$2" pid="$3" timeout="$4" log="$5"
  local waited=0 tty=0
  if [ -t 1 ]; then tty=1; fi
  if [ "$tty" -eq 0 ]; then
    printf '\033[1;36m[delta]\033[0m Waiting for %s on port %s ' "$label" "$port"
  fi
  while true; do
    if port_in_use "$port"; then
      if [ "$tty" -eq 1 ]; then
        printf '\r\033[2K'
        log "$label is listening on port $port (${waited}s)."
      else
        printf ' ready.\n'
      fi
      return 0
    fi
    # The launcher (its subshell) exiting means the port will never come up —
    # cargo build failed, pnpm errored, etc. Stop waiting and let the caller
    # show the log rather than spin until the timeout.
    if ! kill -0 "$pid" 2>/dev/null; then
      if [ "$tty" -eq 1 ]; then printf '\r\033[2K'; else printf ' failed.\n'; fi
      warn "$label exited before listening on port $port. Last 30 lines of $log:"
      tail -n 30 "$log" >&2 2>/dev/null || true
      return 1
    fi
    if [ "$waited" -ge "$timeout" ]; then
      if [ "$tty" -eq 1 ]; then printf '\r\033[2K'; else printf ' timed out.\n'; fi
      warn "$label was not listening on port $port after ${timeout}s. Check $log."
      return 1
    fi
    if [ "$tty" -eq 1 ]; then
      draw_wait_status "$label" "$port" "$waited" "$log"
    else
      printf '.'
    fi
    sleep 1
    waited=$((waited + 1))
  done
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

  # Mint ONE per-run bearer token here, outside both processes, and export the
  # SAME value into the backend and the frontend dev server below. Minting it
  # here (rather than letting the server mint its own) avoids a startup race: the
  # page Vite serves must carry the exact token the server enforces. The server
  # reads DELTA_AUTH_TOKEN; Vite reads it too and injects it into the page as the
  # `delta-auth-token` meta tag (see vite.config.ts), which the frontend then
  # presents on every request. Honor a value the caller already exported.
  local auth_token
  auth_token="${DELTA_AUTH_TOKEN:-$(mint_auth_token)}"
  log "Minted a per-run auth token (the frontend presents it on every request)."

  # --- 1. delta-server. It owns the claude session lifecycle. ---
  ln -sf "$(basename "$SERVER_LOG")" "$SERVER_LOG_LATEST"
  log "Starting delta-server (DELTA_PORT=$DELTA_PORT) ..."
  log "Server log: $SERVER_LOG (latest -> $SERVER_LOG_LATEST)"
  (
    cd "$BACKEND_DIR"
    DELTA_PORT="$DELTA_PORT" \
      DELTA_SESSION_WORKDIR="$workdir" \
      DELTA_TMUX_SOCKET="$DELTA_TMUX_SOCKET" \
      DELTA_AUTH_TOKEN="$auth_token" \
      cargo run -p delta-server >"$SERVER_LOG" 2>&1
  ) &
  local server_pid=$!

  # --- 2. Frontend dev server against the real backend. ---
  ln -sf "$(basename "$FRONTEND_LOG")" "$FRONTEND_LOG_LATEST"
  log "Building frontend workspace libraries and starting the dev server (port $FRONTEND_PORT) ..."
  log "Frontend log: $FRONTEND_LOG (latest -> $FRONTEND_LOG_LATEST)"
  (
    cd "$FRONTEND_DIR"
    export DELTA_AUTH_TOKEN="$auth_token"
    pnpm install >"$FRONTEND_LOG" 2>&1
    pnpm -r build >>"$FRONTEND_LOG" 2>&1
    # `--force` re-optimizes deps so the dev server always serves the libraries
    # just built above. Without it, Vite can keep serving a stale pre-bundled
    # `@delta/*` from its `node_modules/.vite` cache, so a freshly-built library
    # change (e.g. a fix in `@delta/api-client`) silently does not take effect.
    pnpm --filter @delta/web dev -- --force >>"$FRONTEND_LOG" 2>&1
  ) &
  local frontend_pid=$!

  # Return only once both servers are actually reachable, so a freshly-returned
  # `make dev` always means "openable now". The frontend installs and builds the
  # workspace libraries before it binds its port, so it gets a much longer
  # budget than the server. Both are overridable for slow machines / cold caches.
  if ! wait_until_listening "$DELTA_PORT" "delta-server" "$server_pid" \
        "${DELTA_DEV_SERVER_TIMEOUT:-180}" "$SERVER_LOG"; then
    down
    die "delta-server did not come up. See $SERVER_LOG (latest -> $SERVER_LOG_LATEST)."
  fi
  if ! wait_until_listening "$FRONTEND_PORT" "frontend dev server" "$frontend_pid" \
        "${DELTA_DEV_FRONTEND_TIMEOUT:-300}" "$FRONTEND_LOG"; then
    down
    die "frontend dev server did not come up. See $FRONTEND_LOG (latest -> $FRONTEND_LOG_LATEST)."
  fi

  cat <<EOF

────────────────────────────────────────────────────────────────────────────
Delta is up.

  • delta-server  → http://127.0.0.1:$DELTA_PORT  (owns the claude session)
  • frontend      → http://localhost:$FRONTEND_PORT  (ready)

The only manual step: open the browser.

  Open:  http://localhost:$FRONTEND_PORT

On load the UI shows the session list (empty on a fresh database); nothing
auto-starts. The first Send from the composer (or a New action) spawns a fresh
claude session, and Sending to a closed session resumes it. Authentication is
assumed (cached token / CLAUDE_CODE_OAUTH_TOKEN). If claude is not yet
authenticated, run 'claude' once on its own — or attach to a spawned pane — to
complete login, then reload:

       tmux -L $DELTA_TMUX_SOCKET attach -t delta-1   # detach with Ctrl-b then d

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
