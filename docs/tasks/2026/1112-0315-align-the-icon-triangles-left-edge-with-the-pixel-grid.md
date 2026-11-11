---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "grep -q 'translate(20 0) rotate(90 256 256)' assets/icon/delta.svg && ! grep -q 'translate(24 0)' assets/icon/delta.svg && grep -q '^INSET_PERCENT=80$' scripts/gen-icons.sh && scripts/gen-icons.sh && mkdir -p .tmp && find backend/crates/apps/delta-desktop/icons backend/crates/apps/delta-desktop/icons-dev frontend/packages/apps/web/public -maxdepth 1 -type f | sort | xargs sha256sum > .tmp/icons.sha256 && scripts/gen-icons.sh && sha256sum -c --quiet .tmp/icons.sha256"
assignee: null
branch: task/1112-0315-align-the-icon-triangles-left-edge-with-the-pixel-grid
created_at: 2026-11-11T18:02:15Z
updated_at: 2026-11-11T18:12:04Z
---

# fix(desktop): put the icon triangles' left edge on a pixel boundary

## Overview

The macOS `.icns` is the source icon (`assets/icon/delta.svg`) scaled to
80 % and centred on a transparent canvas (`INSET_PERCENT=80` in
`scripts/gen-icons.sh`). The two triangles sit in a group translated by
`translate(24 0)`, which puts their shared left edge at x=120 of the 512-unit
source. Once scaled to 80 % and offset by 51.2 units, that edge falls in the
middle of a pixel at every resolution the `.icns` carries, so one column of
blended colour runs along it and the edge looks soft in the Dock. For
example, at 512 px the column at x=147 is (82,88,110), between the background
`#F4F6F8` and the triangle `#1C2440`, and every resolution of 64 px and up
has such a column.

Moving the group to `translate(20 0)` puts the edge at x=116 of the source,
which lands at 0.28125 of the canvas once inset (from 0.2875 now), a whole
pixel at 32 px and every larger resolution. 16 px cannot be aligned by any
translation, and is left as it is.

What to do:

1. In `assets/icon/delta.svg`, change `translate(24 0)` to
   `translate(20 0)`. Do not change the triangles' paths, the background, or
   `INSET_PERCENT` (75 % would make the Dock icon look smaller than its
   neighbours).
2. Run `make icons` to regenerate every derived file (the installed set
   under `backend/crates/apps/delta-desktop/icons/`, the dev set under
   `icons-dev/`, and `frontend/packages/apps/web/public/favicon.svg` and
   `favicon-32.png`), and commit them. Never draw or edit a PNG, `.ico` or
   `.icns` by hand: they are derived from the SVG by the script alone. The
   script needs `cargo tauri` (`cargo install tauri-cli --version '^2'
   --locked`) and python3.
3. Run `make icons` a second time and confirm no output changes byte for
   byte. If the bytes differ between runs, stop and report it rather than
   working around it: the script promises reproducible output.
4. Verify the fix by unpacking the regenerated `icon.icns` (the installed
   set's) into its PNG entries (the format is a header then
   (4-byte type, 4-byte big-endian length, data) entries; PNG entries start
   with the PNG signature) and, for each resolution of 32 px and up, reading
   the row through the vertical middle of the triangle: the pixel just left
   of the left edge must be the background colour (244,246,248) and the pixel
   just right of it `#1C2440` (28,36,64), with no blended column between.
   Write the values per resolution in the PR description, before and after.
5. Update any document or comment that states the edge position or the
   translation, if one does.

Out of scope: the triangles' shape, `INSET_PERCENT`, the badge geometry.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `assets/icon/delta.svg` translates the group by `translate(20 0)` and
      `INSET_PERCENT` stays 80 (grep gate in `check_command`)
- [x] `make icons` run twice writes the same bytes, and the icon files left
      in the tree are its output from the changed SVG (`check_command` runs it
      twice and compares checksums)

### Before merge (verified outside the check command)

- [x] In every resolution of 32 px and up in the installed set's `icon.icns`,
      the left edge has no blended column: the pixel left of it is
      (244,246,248) and the pixel right of it (28,36,64); the PR description
      lists the values before and after
