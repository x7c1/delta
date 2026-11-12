---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "bash -n scripts/build-assets-car.sh && grep -q '#15315' scripts/build-assets-car.sh && grep -q '#16386' scripts/build-assets-car.sh && grep -q 'icons/icon.icon' scripts/build-assets-car.sh && git diff --quiet origin/main -- backend/crates/apps/delta-desktop/tauri.macos.conf.json Makefile .github"
assignee: null
branch: task/1113-0302-say-when-to-drop-the-assets-car-workaround
created_at: 2026-11-12T18:02:20Z
updated_at: 2026-11-12T18:06:38Z
---

# docs(desktop): say when to drop the prebuilt Assets.car workaround

## Overview

`scripts/build-assets-car.sh` compiles the Icon Composer icon
(`backend/crates/apps/delta-desktop/icons/icon.icon`) into an `Assets.car`
outside the Tauri CLI, because the CLI's own `actool` call fails under
tauri-action on CI (tauri-apps/tauri#15315 and #16386).
`tauri.macos.conf.json` then lists the prebuilt
`../../../target/icon-composer/Assets.car` in `bundle.icon`.

The workaround keeps working after the upstream bugs are fixed, so nothing
will fail to tell a reader it can go. The script's header comment already says
why the workaround exists; it should also say when to remove it and what to
put back, so that a reader of the script notices once the issues are closed.

What to do:

1. In the header comment of `scripts/build-assets-car.sh`, add a short
   paragraph after the one that explains the workaround, saying: once
   tauri-apps/tauri#15315 and #16386 are fixed (the Tauri CLI's `actool` call
   succeeds under tauri-action on CI), delete this script, its call in the
   `Makefile` and in `.github/workflows/bundle.yml`, and
   `tauri.macos.conf.json`, and list `icons/icon.icon` in `bundle.icon` of
   the main Tauri configuration so that the Tauri CLI compiles it itself.
   Check the actual file names and call sites in the repository and name them
   accurately.
2. Change only the comment. Do not change `tauri.macos.conf.json` (JSON cannot
   carry a comment), the `Makefile`, the workflow, or any code.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The header comment of `scripts/build-assets-car.sh` names
      tauri-apps/tauri#15315 and #16386 as the condition for removing the
      workaround
- [x] It lists what to remove and that `icons/icon.icon` goes into
      `bundle.icon` instead, with file names that exist in the repository
- [x] No file other than the script and this task file changes
- [x] `bash -n scripts/build-assets-car.sh` passes
