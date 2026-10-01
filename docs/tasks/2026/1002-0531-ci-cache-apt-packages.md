---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && [ \"$(grep -c 'awalsh128' .github/workflows/ci.yml .github/workflows/bundle.yml | awk -F: '{s+=$2} END {print s}')\" -eq 0 ] && grep -q '/var/cache/apt/archives' .github/workflows/ci.yml && grep -q '/var/cache/apt/archives' .github/workflows/bundle.yml"
assignee: null
branch: task/1002-0531-ci-cache-apt-packages
created_at: 2026-10-01T18:50:11Z
updated_at: 2026-10-01T19:08:10Z
---

# ci: cache the apt packages the workflows install

## Overview

Every CI run installs its system packages from the Ubuntu mirror afresh:
`.github/workflows/ci.yml` installs `tmux` in the `backend` job and `tmux` plus the
Tauri/WebKit build libraries in `e2e-fake`, and `.github/workflows/bundle.yml` installs
the Tauri/WebKit libraries on Linux. When the mirror is slow, those steps alone have
taken over 20 minutes (one Linux bundle run spent 22 minutes in "Install system
packages", downloading `libwebkit2gtk-4.1-0` at a few hundred KB/s), which stretches the
merge gate and once hung a `backend` job for 40 minutes.

Cache the downloaded `.deb` archives so a run normally installs from the cache instead of
the mirror. Use the first-party `actions/cache` on `/var/cache/apt/archives`, keyed by
the runner OS/image and the package list each job installs, and keep installing with
`apt-get install` (so dpkg registers the packages and runs their maintainer scripts and
triggers as before). `apt-get update` still runs, because the index is needed to resolve
current versions; only the large package downloads are saved. Make the archive directory
readable by the cache action after the install (the runner's apt cache is root-owned),
and do not let apt's automatic cleanup empty it before the cache is saved.

Third-party apt-caching actions were considered and rejected: the common one
(`awalsh128/cache-apt-pkgs-action`) downloads and executes an unpinned script from its
repository's default branch on a cache miss, and `bundle.yml` also builds the release
artifacts, so that would run unreviewed code in the job that produces what users
download.

Keep the job- and step-level `timeout-minutes` the workflows already carry.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `ci.yml` and `bundle.yml` cache `/var/cache/apt/archives` with `actions/cache` and
      use no third-party apt-caching action (the `grep` gates in `check_command`).

### Manual / on-hardware (verified by a human before merge)

- [ ] The pull request's CI passes, including the Linux bundle and `e2e-fake`, and a
      second run (re-run or a later push) restores the packages from the cache.
