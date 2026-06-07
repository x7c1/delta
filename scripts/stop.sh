#!/usr/bin/env bash
#
# stop.sh — tear down Delta's local loop.
#
# Stops the `delta-server` process, the frontend dev server, and the `delta`
# tmux session started by scripts/dev.sh. Equivalent to `scripts/dev.sh --down`.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$SCRIPT_DIR/dev.sh" --down
