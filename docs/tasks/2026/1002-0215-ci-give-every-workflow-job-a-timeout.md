---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && for f in .github/workflows/*.yml; do [ \"$(grep -c '^    runs-on:' $f)\" -eq \"$(grep -c '^    timeout-minutes:' $f)\" ] || { echo \"missing job timeout in $f\"; exit 1; }; done"
assignee: null
branch: task/1002-0215-ci-give-every-workflow-job-a-timeout
created_at: 2026-10-01T16:40:58Z
updated_at: 2026-10-01T16:51:36Z
---

# ci: give every workflow job a timeout

## Overview

No job in `.github/workflows/` sets `timeout-minutes`, so a hung step runs until
GitHub's 6-hour default. This happened on a pull request: the CI `backend` job sat in
its "Install tmux" step (`apt-get`) for 40 minutes without progress, holding the
merge gate open while every other check had long finished, and nothing reported it.

Give every job in every workflow (`ci.yml`, `bundle.yml`, `release.yml`,
`create-release-pr.yml`, `validate-release-pr.yml`) a job-level `timeout-minutes`
so a hang fails the job instead of stalling it. Pick each limit from the job's usual
duration with generous headroom (roughly 2–3x), so a slow but healthy run never trips
it. Recent durations for reference: CI `backend` ~2–3 min, `frontend` ~2–7 min,
`e2e-fake` 8–23 min (the longer runs are cold caches), `bundle` 5–10 min per
platform, `validate-title` seconds. Read the release workflows to judge theirs.

Where a single step is known to hang on the network (the `apt-get` installs), a
step-level `timeout-minutes` as well is welcome but not required.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Every job in every workflow under `.github/workflows/` has a job-level
      `timeout-minutes` (the per-file count gate in `check_command`: as many
      `    timeout-minutes:` lines at job indentation as `    runs-on:` lines).

### Manual / on-hardware (verified by a human before merge)

- [ ] The pull request's own CI run passes with the new limits (no healthy job is
      cut off).
