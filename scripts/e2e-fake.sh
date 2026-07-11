#!/usr/bin/env bash
#
# e2e-fake.sh — run the fake-mode Playwright suite against a real backend.
#
# This script is a thin wrapper: it builds the two binaries the suite needs
# and invokes Playwright. The `delta-server` lifecycle itself — the temp
# database, the per-run tmux socket, the scripted-claude wrapper, the spawn,
# the `/health` readiness poll, and teardown — is owned by a worker-scoped
# Playwright fixture (frontend/packages/apps/web/e2e-fake/support/server.ts),
# NOT by this script. That single Node boot implementation is what lets a spec
# kill the server and relaunch it (the server-restart coverage) and keeps the
# two entry points from drifting.
#
# The suite drives the real frontend against a real `delta-server` whose
# spawned "claude" is the scripted `fake-claude` binary. Everything between the
# browser and the scripted model is real: REST, the WebSocket event channel,
# the PTY bridge, tmux panes, hooks, and the JSONL transcript tail.
#
# Usage: scripts/e2e-fake.sh [playwright args...]
#   Any trailing arguments are forwarded to `playwright test`, so a single
#   spec or filter can be run against the same real harness, e.g.
#     scripts/e2e-fake.sh server-restart.spec.ts
#   E2E_FAKE_BACKEND_PORT / E2E_FAKE_PORT override the ports (the config and
#   the server fixture both read E2E_FAKE_BACKEND_PORT).
#
# Prerequisites: tmux, the Rust toolchain, pnpm (workspace installed and
# libraries built: `pnpm install && pnpm -r build` in frontend/), and the
# Playwright chromium browser.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BACKEND_DIR="$REPO_ROOT/backend"
FRONTEND_DIR="$REPO_ROOT/frontend"

log() { printf '\033[1;36m[e2e-fake]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[e2e-fake]\033[0m %s\n' "$*" >&2; exit 1; }

command -v tmux >/dev/null 2>&1 || die "tmux not found on PATH"
command -v cargo >/dev/null 2>&1 || die "cargo not found on PATH"
command -v pnpm >/dev/null 2>&1 || die "pnpm not found on PATH"

# Build both binaries up front so the fixture's server spawn is instant (the
# fixture launches the built `target/debug/{delta-server,fake-claude}`).
log "Building delta-server and fake-claude ..."
(cd "$BACKEND_DIR" && cargo build -p delta-server -p fake-claude)

# Run the suite; the worker fixture boots and tears down the server, and
# Playwright owns the Vite dev server (proxied to the backend port).
log "Running the fake-mode Playwright suite ..."
(
  cd "$FRONTEND_DIR"
  pnpm --filter @delta/web e2e:fake "$@"
)
