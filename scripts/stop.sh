#!/usr/bin/env bash
#
# stop.sh — tear down Delta's local walking skeleton.
#
# Kills the `delta-server` process and the `delta` tmux session started by
# scripts/dev.sh. Equivalent to `scripts/dev.sh --down`.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$SCRIPT_DIR/dev.sh" --down
