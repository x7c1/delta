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

## reset: stop the loop and reset the database (empty schema on next start)
.PHONY: reset
reset:
	scripts/dev.sh --reset

# --- Generated code -----------------------------------------------------------

## gen: regenerate the TypeScript wire bindings (@delta/wire-gen) from the Rust wire contract
.PHONY: gen
gen:
	cd backend && cargo run -p delta-wire --bin export-ts

## gen-check: fail when the committed @delta/wire-gen bindings are stale (regenerate + diff)
.PHONY: gen-check
gen-check: gen
	@if [ -n "$$(git status --porcelain -- frontend/packages/gateway/wire-gen)" ]; then \
		git --no-pager diff -- frontend/packages/gateway/wire-gen; \
		echo "error: generated wire bindings are stale — run 'make gen' and commit the result"; \
		exit 1; \
	fi

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

## check: full pre-PR gate — backend build/test/clippy + generated-bindings freshness + frontend build/typecheck/test/lint
.PHONY: check
check:
	cd backend && cargo build && cargo test && cargo clippy --all-targets -- -D warnings
	$(MAKE) gen-check
	cd frontend && pnpm -r build && pnpm -r typecheck && pnpm -r test && pnpm -r lint

## e2e: run the headless Playwright suite (one-time: `pnpm --filter @delta/web exec playwright install --with-deps chromium`)
# Pin a dedicated mock-server port so the suite never collides with a dev server
# on the default 5173 (and never adopts a live, real-backend one — see
# playwright.config.ts reuseExistingServer).
.PHONY: e2e
e2e:
	cd frontend && E2E_PORT=5199 pnpm --filter @delta/web e2e

## e2e-fake: run the fake-mode Playwright suite — real backend + tmux with the scripted fake-claude binary (requires tmux)
.PHONY: e2e-fake
e2e-fake:
	scripts/e2e-fake.sh

## e2e-real: run the real-claude canary suite — contract monitoring against the real `claude` CLI (local only; consumes Claude quota; never in CI)
.PHONY: e2e-real
e2e-real:
	scripts/e2e-real.sh

## e2e-real-auto: run e2e-real only if the claude version changed AND ≥24h since the last attempt — for a periodic driver (see docs/guides/development.md)
.PHONY: e2e-real-auto
e2e-real-auto:
	scripts/e2e-real-auto.sh
