# Delta — unified entry point.
#
# Run `make help` (the default) for the full target list. Every target wraps an
# existing entry point (scripts/dev.sh or the per-part cargo/pnpm commands) so
# the repo has one place to run things from: the repo root.

# Working directory passed to scripts/dev.sh. Empty by default so dev.sh uses
# its own default (.tmp/session). Override per-invocation: `make dev WORKDIR=~/scratch`.
WORKDIR ?=

# Optional host-specific overrides (gitignored). Use it to `export` env vars every
# target should inherit — e.g. a linker override on hosts where the default `cc`
# cannot link macOS binaries. See local.mk.example. Missing file is fine (`-`).
-include local.mk

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

# --- Serve the built frontend from delta-server --------------------------------

## web-dist: build the web SPA (and the workspace libraries it needs) into frontend/packages/apps/web/dist for the embedded server
# Mock mode and a foreign API base are dev-only knobs: unset them so an exported
# value in the caller's shell cannot leak into the build the server embeds.
.PHONY: web-dist
web-dist:
	cd frontend && env -u VITE_API_MOCK -u VITE_API_BASE_URL pnpm --filter "@delta/web..." build

## server-embedded: release-build delta-server with the built SPA compiled in (`embed-web`); serves the app at http://127.0.0.1:7878/
.PHONY: server-embedded
server-embedded: web-dist
	cd backend && cargo build -p delta-server --features embed-web --release

# --- Desktop app (Tauri shell around the embedded server) ---------------------

## app-dev: build the SPA (make web-dist), then run the desktop shell from source (`cargo run -p delta-app`)
.PHONY: app-dev
app-dev: web-dist
	cd backend && cargo run -p delta-app

## app: build the SPA, then bundle the desktop shell under backend/target/release/bundle/ (macOS: Delta.app and a .dmg; Linux: .deb, .rpm and an AppImage) (one-time: `cargo install tauri-cli --version '^2' --locked`)
.PHONY: app
app: web-dist
	cd backend/crates/apps/delta-app && cargo tauri build

# --- Generated code -----------------------------------------------------------

## gen: regenerate the TypeScript wire bindings (@delta/wire-gen) from the Rust wire contract
.PHONY: gen
gen:
	cd backend && cargo run -p delta-wire --bin export-ts

## gen-check: fail when the @delta/wire-gen bindings on disk differ from what the Rust contract generates now (generates into a temp dir and diffs; writes nothing, ignores git; part of `make check` and of CI's backend job)
.PHONY: gen-check
gen-check:
	scripts/gen-check.sh

## gen-check-test: exercise gen-check against a stub generator and a throwaway git repo (needs no cargo; part of `make check`)
.PHONY: gen-check-test
gen-check-test:
	bash scripts/tests/gen-check.test.sh

# --- Vendored codex app-server schema -----------------------------------------

## vendor-codex-schema: re-vendor backend/crates/gateway/codex-agent/vendor/app-server-schema from the installed codex (DELTA_CODEX_BIN overrides the binary)
.PHONY: vendor-codex-schema
vendor-codex-schema:
	scripts/vendor-codex-schema.sh

## vendor-codex-schema-check: fail when a vendored codex schema file is not in its pinned key-sorted form (needs only jq; part of `make check` and of CI's backend job)
.PHONY: vendor-codex-schema-check
vendor-codex-schema-check:
	scripts/vendor-codex-schema.sh --check

## vendor-codex-schema-test: exercise the re-vendor script against a stub generator in a throwaway directory (needs no codex; part of `make check`)
.PHONY: vendor-codex-schema-test
vendor-codex-schema-test:
	bash scripts/tests/vendor-codex-schema.test.sh

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

## lint: rustfmt + clippy (backend) + eslint & dependency-cruiser (frontend)
.PHONY: lint
lint:
	cd backend && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings
	cd frontend && pnpm -r lint

## check: full pre-PR gate — everything CI runs: backend fmt/build/test/clippy + generated-bindings freshness + vendored-schema form + frontend build/typecheck/test/lint + the embedded-server build (`embed-web`) + the desktop-shell build/test/clippy + both Playwright suites (needs tmux) — plus the canary gate's, the re-vendor script's and gen-check's own stubbed tests, which CI does not run. A dependency graph: `make -j4 check` runs independent steps concurrently (add `-O` to keep each step's output together)
# The point of this target is that passing it means CI will pass, so it has to
# stay a superset of what the workflow runs — including BOTH Playwright suites.
# `e2e` is the mock-backed one and `e2e-fake` drives the real backend through
# tmux with the scripted fake agent binaries; they run different specs, so
# leaving either out lets a suite fail in CI that a green local gate claimed to
# cover. For a quick inner-loop check, reach for `build` / `test` / `lint`
# instead — those stay fast on purpose.
#
# The steps are the `check-*` targets below, wired by what each one needs, so
# `make -j check` runs the backend column, the frontend column and the
# standalone script checks side by side:
#
#   check-backend-fmt → check-backend-build → { check-backend-test, check-backend-clippy, check-gen }
#   check-frontend-build → { check-frontend-typecheck, check-frontend-test, check-frontend-lint, check-e2e }
#   { check-backend-build, check-frontend-build } → { check-e2e-fake, check-embedded, check-app-build }
#   vendor-codex-schema-check, vendor-codex-schema-test, e2e-real-gate-test, gen-check-test (no prerequisites)
#
# Without -j, make walks the same graph left to right, which is the serial
# order the gate always had. The cargo steps share backend/target and take
# cargo's own lock on it, so they queue behind each other rather than collide;
# the pnpm steps share nothing cargo writes. The two Playwright suites run on
# their own ports (e2e: 5199; e2e-fake: 5198 + backend 7899, per-run tmux
# socket and temp DB) and write separate output dirs (test-results/e2e/ and
# test-results/e2e-fake/). The standalone targets (`make e2e`, `make e2e-fake`,
# `make gen-check`, …) keep working on their own: the check-* wrappers add the
# ordering, not the behaviour.
CHECK_STEPS := \
	check-backend-test check-backend-clippy check-gen \
	vendor-codex-schema-check vendor-codex-schema-test e2e-real-gate-test gen-check-test \
	check-frontend-typecheck check-frontend-test check-frontend-lint \
	check-e2e check-e2e-fake check-embedded check-app-build

.PHONY: check $(CHECK_STEPS) check-backend-fmt check-backend-build check-frontend-build
check: $(CHECK_STEPS)

check-backend-fmt:
	cd backend && cargo fmt --all -- --check

check-backend-build: check-backend-fmt
	cd backend && cargo build

check-backend-test: check-backend-build
	cd backend && cargo test

check-backend-clippy: check-backend-build
	cd backend && cargo clippy --all-targets -- -D warnings

# `gen-check` runs export-ts through `cargo run`, which reuses this build.
check-gen: check-backend-build
	$(MAKE) gen-check

check-frontend-build:
	cd frontend && pnpm -r build

check-frontend-typecheck: check-frontend-build
	cd frontend && pnpm -r typecheck

check-frontend-test: check-frontend-build
	cd frontend && pnpm -r test

check-frontend-lint: check-frontend-build
	cd frontend && pnpm -r lint

check-e2e: check-frontend-build
	$(MAKE) e2e

check-e2e-fake: check-backend-build check-frontend-build
	$(MAKE) e2e-fake

# Builds delta-server with `embed-web` against the `dist/` check-frontend-build
# just produced, so the compile-time include and the static routes are proven
# against a real build on every gate. Debug is enough for that.
check-embedded: check-backend-build check-frontend-build
	cd backend && cargo build -p delta-server --features embed-web

# The desktop shell is outside the workspace's default members, so only this
# step builds, tests and lints it. It embeds the same `dist/` through
# delta-server's `embed-web`. Debug is enough.
check-app-build: check-backend-build check-frontend-build
	cd backend && cargo build -p delta-app && cargo test -p delta-app && cargo clippy -p delta-app --all-targets -- -D warnings

## e2e: run the headless Playwright suite (one-time: `pnpm --filter @delta/web exec playwright install --with-deps chromium`)
# Pin a dedicated mock-server port so the suite never collides with a dev server
# on the default 5173 (and never adopts a live, real-backend one — see
# playwright.config.ts reuseExistingServer).
.PHONY: e2e
e2e:
	cd frontend && E2E_PORT=5199 pnpm --filter @delta/web e2e

## e2e-fake: run the fake-mode Playwright suite — real backend + tmux with the scripted fake-claude / fake-codex binaries (requires tmux)
.PHONY: e2e-fake
e2e-fake:
	scripts/e2e-fake.sh

## e2e-real-claude: run the real-claude canary suite — contract monitoring against the real `claude` CLI (local only; consumes Claude quota; never in CI)
.PHONY: e2e-real-claude
e2e-real-claude:
	scripts/e2e-real-claude.sh

## e2e-real-gate: run e2e-real-claude and e2e-real-codex, each only if that CLI's version changed AND ≥24h since its own last attempt — for a periodic driver (see docs/guides/development/canary.md)
.PHONY: e2e-real-gate
e2e-real-gate:
	scripts/e2e-real-gate.sh

## e2e-real-gate-test: exercise the canary gate's decision paths with stub CLIs and a stub suite (no quota, no network; part of `make check`)
.PHONY: e2e-real-gate-test
e2e-real-gate-test:
	bash scripts/tests/e2e-real-gate.test.sh

## e2e-real-codex: run the real-codex canaries against the real `codex app-server` — one safe turn end-to-end + the thread-metadata wire fields + the worktree sandbox git grant + schema drift detection (local only; only the turn consumes Codex quota; never in CI). DELTA_CODEX_BIN overrides the binary.
.PHONY: e2e-real-codex
e2e-real-codex:
	cd backend && cargo test -p codex-agent --test real_codex_canary -- --ignored --test-threads=1 --nocapture
