---
status: completed
pipeline_phase: null
plan: null
follow_up_of: docs/tasks/2026/0912-1146-feat-refuse-to-start-without-tmux.md
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -q ''probes for it at startup'' .github/workflows/ci.yml && grep -q ''its failure mode there is a skip, not a spawn error'' backend/crates/apps/fake-claude/tests/full_loop.rs'
assignee: null
branch: task/0912-1330-follow-up-feat-refuse-to-start-without-tmux
created_at: 2026-09-12T13:30:51Z
updated_at: 2026-09-12T15:45:29Z
---

# docs: say why CI installs tmux now that the server requires it at startup

## Overview

Two comments about tmux became stale when the server started refusing to boot
without `tmux` on `PATH`. Both describe tmux as something only the
fake-claude full-loop test needs, and both say a run without tmux merely
skips; now every test that wires the production composition root fails
without it. Comment text only; no behaviour changes.

- `.github/workflows/ci.yml`, lines 34-36 — the three comment lines directly
  above `- name: Install tmux`, beginning
  `# The fake-claude full-loop integration test drives a real tmux server;`.
  Replace them with:

  ```
      # tmux is a hard prerequisite for this job, not a nicety: the composition
      # root probes for it at startup, so every test that wires the production
      # root fails without it. The fake-claude full-loop test, which drives a
      # real tmux server, merely skips.
  ```

- `backend/crates/apps/fake-claude/tests/full_loop.rs`, lines 11-12 —
  `//! Requires a \`tmux\` on \`PATH\`; the test skips (with a note) where tmux is`
  / `//! absent so the workspace test suite stays runnable everywhere. CI installs`.
  Replace those two lines with:

  ```
  //! Requires a `tmux` on `PATH`; the test skips (with a note) where tmux is
  //! absent, so its failure mode there is a skip, not a spawn error. CI installs
  ```

  The following line, `//! tmux explicitly so the loop is always exercised there.`,
  is unchanged.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The CI comment above `Install tmux` explains that the composition root
      probes for tmux at startup:
      `grep -q 'probes for it at startup' .github/workflows/ci.yml`.
- [x] The full-loop test's module doc no longer claims the whole workspace
      suite runs without tmux:
      `grep -q 'its failure mode there is a skip, not a spawn error' backend/crates/apps/fake-claude/tests/full_loop.rs`.
