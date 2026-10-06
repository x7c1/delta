#!/usr/bin/env bash
#
# sweep-test-residue.sh — clean up the dead runs a test harness left behind.
#
# Every harness that boots a real tmux (and, for the browser lanes, a real
# delta-server) calls this once at the start of a run, with its context. The
# rule, written once here:
#
#   - a run whose owner process is alive is never touched;
#   - a dead run's tmux server is killed and its socket file unlinked
#     (`tmux kill-server` leaves the file behind, so even a clean run leaves
#     one per run);
#   - of the dead run directories, the newest is kept as evidence (its
#     database and server log are what a failure investigation reads) and the
#     rest are removed.
#
# Contexts and the names they own:
#
#   context    socket name                  run directory
#   e2e-fake   delta-e2e-fake-<pid>         $TMPDIR/delta-e2e-fake.*
#   e2e-real   delta-e2e-real-<pid>         $TMPDIR/delta-e2e-real.*
#   fake-test  delta-fake-test-<pid>        (tempfile, cleaned on drop)
#   canary     delta-canary-<name>-<pid>    (under CARGO_TARGET_TMPDIR)
#
# A socket's owner is the pid at the end of its name; a run directory's owner
# is the pid in its `owner.pid` file, written by the harness at creation. A
# directory without `owner.pid` counts as dead (it predates the file). Owner
# liveness is `kill -0`, so pid reuse can only make the sweep skip something,
# never touch a live run. Only the prefixes above are ever matched: the
# production and dev sockets (`io.github.x7c1.delta*`), `default`, and
# anything else are never candidates.
#
# A dead run directory may also list server pids in `server.pids` (a harness
# killed hard can orphan its delta-server, which would keep holding the
# lane's port); any of those still alive AND still a `delta-server` is killed.
#
# Usage: scripts/sweep-test-residue.sh <e2e-fake|e2e-real|fake-test|canary>
#   TMUX_TMPDIR  where tmux keeps its socket directory (default /tmp)
#   TMPDIR       where run directories are created (default /tmp)
#
# Prints one line per action and exits 0 even when nothing is found.

set -euo pipefail

log() { printf '[sweep-test-residue] %s\n' "$*"; }

usage() {
  printf 'usage: %s <e2e-fake|e2e-real|fake-test|canary>\n' "$0" >&2
  exit 2
}

[ $# -eq 1 ] || usage
CONTEXT="$1"

case "$CONTEXT" in
  e2e-fake) SOCKET_PREFIX="delta-e2e-fake-"; RUN_PREFIX="delta-e2e-fake." ;;
  e2e-real) SOCKET_PREFIX="delta-e2e-real-"; RUN_PREFIX="delta-e2e-real." ;;
  fake-test) SOCKET_PREFIX="delta-fake-test-"; RUN_PREFIX="" ;;
  canary) SOCKET_PREFIX="delta-canary-"; RUN_PREFIX="" ;;
  *) usage ;;
esac

SOCKET_DIR="${TMUX_TMPDIR:-/tmp}"
SOCKET_DIR="${SOCKET_DIR%/}/tmux-$(id -u)"
TEMP_DIR="${TMPDIR:-/tmp}"
TEMP_DIR="${TEMP_DIR%/}"
[ -n "$TEMP_DIR" ] || TEMP_DIR="/"

# Whether $1 is a positive integer naming a live process.
owner_alive() {
  case "$1" in
    '' | *[!0-9]*) return 1 ;;
  esac
  [ "$1" -gt 0 ] || return 1
  kill -0 "$1" 2>/dev/null
}

# --- tmux sockets ---------------------------------------------------------------

if [ -d "$SOCKET_DIR" ]; then
  for socket in "$SOCKET_DIR/$SOCKET_PREFIX"*; do
    [ -e "$socket" ] || continue
    name="${socket##*/}"
    owner="${name##*-}"
    case "$owner" in
      '' | *[!0-9]*)
        log "skip socket $name: no owner pid at the end of its name"
        continue
        ;;
    esac
    if owner_alive "$owner"; then
      continue
    fi
    # The server may already be gone (a crashed run, or only the stale file
    # `kill-server` leaves behind); that is the expected failure.
    if tmux -L "$name" kill-server >/dev/null 2>&1; then
      log "killed tmux server $name (owner $owner is dead)"
    fi
    rm -f "$socket"
    log "removed socket $socket"
  done
fi

# --- run directories ------------------------------------------------------------

[ -n "$RUN_PREFIX" ] || exit 0

# Kill any delta-server a dead run recorded that is still running.
kill_orphaned_servers() {
  [ -f "$1/server.pids" ] || return 0
  while IFS= read -r pid || [ -n "$pid" ]; do
    owner_alive "$pid" || continue
    comm="$(ps -p "$pid" -o comm= 2>/dev/null || true)"
    case "$comm" in
      *delta-server*)
        kill -9 "$pid" 2>/dev/null || true
        log "killed orphaned delta-server $pid of $1"
        ;;
    esac
  done <"$1/server.pids"
}

kept=""
# Newest first; `ls -t` sorts by modification time on both GNU and BSD.
# shellcheck disable=SC2012
while IFS= read -r dir; do
  [ -d "$dir" ] || continue
  owner=""
  [ -f "$dir/owner.pid" ] && owner="$(tr -d '[:space:]' <"$dir/owner.pid")"
  if owner_alive "$owner"; then
    continue
  fi
  kill_orphaned_servers "$dir"
  if [ -z "$kept" ]; then
    kept="$dir"
    log "kept $dir as the latest dead run's evidence"
    continue
  fi
  rm -rf "$dir"
  log "removed run directory $dir"
done < <(ls -1dt "$TEMP_DIR/$RUN_PREFIX"* 2>/dev/null || true)

exit 0
