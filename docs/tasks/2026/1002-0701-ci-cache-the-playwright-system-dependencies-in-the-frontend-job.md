---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && python3 -c \"import yaml,sys; steps=yaml.safe_load(open('.github/workflows/ci.yml'))['jobs']['frontend']['steps']; sys.exit(0 if any('/var/cache/apt/archives' in str(st.get('with',{}).get('path','')) for st in steps) else 1)\""
assignee: null
branch: task/1002-0701-ci-cache-the-playwright-system-dependencies-in-the-frontend-job
created_at: 2026-10-02T04:11:46Z
updated_at: 2026-10-02T04:19:30Z
---

# ci: cache the Playwright system dependencies in the frontend job

## Overview

The CI `frontend` job runs `pnpm --filter @delta/web exec playwright install --with-deps
chromium` (`.github/workflows/ci.yml`, "Install Playwright browser"), which installs
Chromium's system libraries with `apt-get`. When the Ubuntu mirror is slow that step
alone takes minutes (one run fetched 32.1 MB at 113 kB/s, 4 min 43 s).

The `e2e-fake` job in the same workflow already caches `/var/cache/apt/archives/*.deb`
with `actions/cache` (see the explanatory comment in that job): there the Playwright
step now reports "Need to get 0 B" because its archives come from that cache. Give the
`frontend` job the same treatment, following the e2e-fake job's steps and conventions
(prepare step, cache step keyed by runner OS, image and what is installed, readable
archives after the install, the apt clean hook removed). The `frontend` job installs no
apt packages of its own, so the key must reflect what Playwright installs — for example
the Playwright version from the lockfile — so that a Playwright upgrade does not keep
restoring a stale set. Keep the explanation in the e2e-fake job as the single source and
point to it, as `bundle.yml` already does, and keep every existing `timeout-minutes`.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The `frontend` job in `ci.yml` caches `/var/cache/apt/archives` (the `yaml` gate in
      `check_command`: some step of the job caches that path).

### Manual / on-hardware (verified by a human before merge)

- [ ] The pull request's CI passes, and a second run's `frontend` job restores the
      archives and its Playwright step reports "Need to get 0 B".
