# Delta — unified entry point.
#
# Run `make help` (the default) for the full target list. Every target wraps an
# existing entry point (scripts/dev.sh or the per-part cargo/pnpm commands) so
# the repo has one place to run things from: the repo root.

# Working directory passed to scripts/dev.sh. Empty by default so dev.sh uses
# its own default (.tmp/session). Override per-invocation: `make dev WORKDIR=~/scratch`.
WORKDIR ?=

.DEFAULT_GOAL := help

## help: list available targets
.PHONY: help
help:
	@awk '/^## / { sub(/^## /, ""); i = index($$0, ": "); printf "  \033[36m%-8s\033[0m %s\n", substr($$0, 1, i - 1), substr($$0, i + 2) }' $(MAKEFILE_LIST)

# --- Run the full local loop (backend + frontend + claude/tmux) ---------------

## dev: start the full local loop (server + web dev server); WORKDIR overrides the claude workdir
.PHONY: dev
dev:
	scripts/dev.sh $(WORKDIR)

## mock: frontend-only mock-data mode (no backend/tmux/claude) at http://localhost:5173
.PHONY: mock
mock:
	cd frontend && pnpm -r build && VITE_API_MOCK=1 pnpm --filter @delta/web dev --force

## down: stop the local loop (server, web dev server, spawned tmux sessions)
.PHONY: down
down:
	scripts/dev.sh --down

## reset: stop the loop and clear local state (db + session workdirs)
.PHONY: reset
reset:
	scripts/dev.sh --reset

# --- Quality gate -------------------------------------------------------------

## build: build backend and frontend
.PHONY: build
build:
	cd backend && cargo build
	cd frontend && pnpm -r build

## test: run backend and frontend tests
.PHONY: test
test:
	cd backend && cargo test
	cd frontend && pnpm -r test

## lint: clippy (backend) + eslint & dependency-cruiser (frontend)
.PHONY: lint
lint:
	cd backend && cargo clippy --all-targets -- -D warnings
	cd frontend && pnpm -r lint

## check: full pre-PR gate — backend build/test/clippy + frontend build/typecheck/test/lint
.PHONY: check
check:
	cd backend && cargo build && cargo test && cargo clippy --all-targets -- -D warnings
	cd frontend && pnpm -r build && pnpm -r typecheck && pnpm -r test && pnpm -r lint

## e2e: run the headless Playwright suite (one-time: `pnpm --filter @delta/web exec playwright install --with-deps chromium`)
.PHONY: e2e
e2e:
	cd frontend && pnpm --filter @delta/web e2e
