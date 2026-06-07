#!/usr/bin/env bash
#
# reset.sh — tear down Delta's local loop and reset the database.
#
# Stops the `delta-server` process, the frontend dev server, and the `delta`
# tmux session, then deletes the SQLite database so the next `scripts/dev.sh`
# starts from an empty schema. Equivalent to `scripts/dev.sh --reset`.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$SCRIPT_DIR/dev.sh" --reset
