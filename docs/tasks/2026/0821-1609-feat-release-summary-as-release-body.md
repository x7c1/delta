---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && bash .github/scripts/release-summary.test.sh"
assignee: null
branch: task/0821-1609-feat-release-summary-as-release-body
created_at: 2026-08-21T16:09:50Z
updated_at: 2026-08-21T17:35:00Z
---

# feat(release): publish a hand-written summary as the GitHub Release body

## Overview

The GitHub Release and the release PR currently carry the same
machine-generated body: `release.yml` and `create-release-pr.sh` each call
`generate-changelog.sh` independently, so both end up as one flat list of
every commit since the last tag, bucketed by conventional-commit type.

That single artifact is being asked to serve two different readers, and it
serves neither well. For the developer the itemized PR list is a usable
ledger — that side is fine and this task does not change it. For someone
reading the GitHub Release it is close to unreadable: the v0.4.0 body runs to
75 entries, a third of which are `refactor` / `docs` / `test` / `build` / `ci`
items with no reader-visible effect, rendered at the same heading level as the
features; the one commit without a conventional-commit prefix — which happens
to be the release's headline change — lands at the bottom under "Other
Changes"; and there is no summary of what the release *is* anywhere, because
nothing in a commit log can produce one.

Split the two readers apart:

- **The release PR stays exactly as it is** — the auto-generated, type-bucketed
  list of every commit. It is the developer-facing ledger.
- **The GitHub Release becomes a hand-written summary plus links** to that
  ledger and to the compare view.

The summary has to be authored by a human, so it needs somewhere to live that
`release.yml` can read at tag time. Put it in the release PR body itself,
above a marker, and preserve it across regeneration — exactly as
`create-release-pr.sh` already preserves the developer-chosen title. This
keeps the release PR as the single moving handle a developer touches
(`create-release-pr.sh`'s own header comment states that design), rather than
introducing a second handle in the form of a separate file that has to be
merged to `main` before the release PR can go in.

Note that a file committed on the release branch is not an option: on every
push to `main`, `update_release_pr` rebuilds the branch with
`git checkout -B "$branch" origin/main` and force-pushes, so any hand-added
commit on that branch is discarded.

### Body layout

The release PR body gains a summary region above a marker line:

```markdown
## Summary

<!-- release-summary:todo -->
_Write the user-facing release summary here. It becomes the body of the
GitHub Release. Delete the marker comment above once written._

<!-- changelog:auto -->

## Features
...
```

Everything **above** `<!-- changelog:auto -->` is the human region, preserved
verbatim across regeneration. Everything from the marker down is regenerated
from `git log` on every push to `main`, as it is today.

### Resulting GitHub Release body

```markdown
<the human region, verbatim, with the marker line itself removed>

---

- Full changelog: https://github.com/x7c1/delta/pull/<release PR number>
- Compare: [v0.3.0...v0.4.0](https://github.com/x7c1/delta/compare/v0.3.0...v0.4.0)
```

The compare link is what the Release body already carries today; keep it.

## Work

### 1. New shared script: `.github/scripts/release-summary.sh`

Sourceable, following the existing `generate-changelog.sh` pattern (function
definitions plus a standalone-execution guard). It must contain **only pure
text functions** — no `gh`, no network — so the test script below can exercise
them directly:

- `changelog_marker()` — echoes the single marker string `<!-- changelog:auto -->`
  so no caller hardcodes it a second time.
- `summary_region <body>` — echoes everything above the first occurrence of the
  marker, with trailing blank lines trimmed. When the marker is absent, echoes
  nothing (see the fallback rule below).
- `summary_placeholder()` — echoes the `## Summary` template block shown above,
  including the `<!-- release-summary:todo -->` sentinel.
- `summary_is_unwritten <summary>` — true when the summary is unusable as a
  release body: empty, or whitespace-only, or still containing the
  `<!-- release-summary:todo -->` sentinel, after HTML comments are stripped.

Split on the exact marker string, not a regular expression, so arbitrary
Markdown in the human region (code fences, nested comments, `---` rules)
cannot corrupt the split.

**Marker-absent fallback.** A body predating this change has no marker — the
currently open release PR is exactly that case. Treat a missing marker as "no
summary written yet" and use `summary_placeholder` when regenerating. Nothing
is lost: every such body is wholly machine-generated and the changelog beneath
is rebuilt in full anyway.

### 2. `create-release-pr.sh` — preserve the summary region

- `create_release_pr` (first run): body is `summary_placeholder` + marker +
  changelog + compare link.
- `update_release_pr` (every subsequent push to `main`): read the existing body
  with `gh pr view "$pr_number" --json body --jq '.body'`, take
  `summary_region` of it, and re-emit it verbatim above the marker. Fall back
  to `summary_placeholder` when the region is absent or empty.
- `render_pr_body` grows a summary parameter; the compare-link behaviour is
  unchanged.

`update-release-pr-links.sh` rewrites the compare link with `sed` over the
whole body after the release lands. Confirm its substitution
(`...${branch}` → `...v${version}`) cannot match inside a summary region — and
if it can, scope the rewrite to the changelog region using the same split
rather than leaving the hazard in place.

### 3. `release.yml` — summary plus links, and fail loudly without one

Replace the `Generate changelog` step. The new step must run **before**
`Create GitHub release`, so a missing summary stops the run before a tag
exists:

1. Find the merged release PR for `$NEW_VERSION`. `update-release-pr-links.sh`
   already does this with `find_release_pr`; move that function into a new
   sourceable `.github/scripts/release-pr-lookup.sh` that both
   `update-release-pr-links.sh` and this workflow use, instead of writing a
   second copy — one implementation, two callers. It goes there rather than
   into `release-summary.sh` because it calls `gh`, and `release-summary.sh`
   stays pure so its functions can be unit-tested without the network.
2. Read its body, take `summary_region`.
3. If the PR is not found, or `summary_is_unwritten` reports true, fail the
   step with `::error::` and a message naming what to do. Do **not** silently
   fall back to the generated changelog — a silent fallback would restore
   exactly the unreadable body this task removes.
4. Emit the summary, a `---` rule, the full-changelog link to the release PR,
   and the existing compare link.

`generate-changelog.sh` is then called only by `create-release-pr.sh`. Leave
the script itself unchanged.

### 4. `validate-release-pr.yml` — gate the summary before merge

The workflow already pre-filters on a `Release v` title and checks out the
repo. Add a step, under the same gate, that runs a new
`.github/scripts/validate-release-summary.sh` against
`github.event.pull_request.body`, failing when `summary_is_unwritten` is true
with a message naming the marker and what to write. The validator takes the
body as an argument or on stdin and calls into `release-summary.sh` — no `gh`,
so it is unit-testable.

This workflow triggers on `edited`, so writing the summary re-runs the check
and turns it green without any other push.

### 5. `.github/scripts/release-summary.test.sh`

A plain bash test that sources `release-summary.sh` and asserts on fixture
bodies. It is appended to `check_command`, so it runs in the same gate as the
rest of the suite. Cover at least:

- `summary_region` returns the human text for a body containing the marker,
  and excludes the marker line and everything below it.
- A summary region containing a fenced code block that itself contains the
  marker string still splits at the *first* occurrence.
- `summary_region` on a marker-less body returns empty.
- `summary_is_unwritten` is true for empty, whitespace-only, and
  sentinel-bearing summaries, and false for a written one.
- A summary whose prose contains the word `TODO` in ordinary text (not the
  sentinel comment) is **not** treated as unwritten.

Every assertion must fail when its expectation is inverted; verify that while
writing them, so no assertion is vacuously green.

### 6. `docs/guides/release.md`

Update to match. At minimum: step 2 and 3 of "Normal flow" (the Release no
longer carries the same changelog as the PR), a new short section describing
the summary region, the marker, and that the release cannot be merged with the
summary unwritten, and the `Workflows involved` entries for `release.yml` and
`validate-release-pr.yml`.

Follow the repo's DRY rule: the marker string and body layout are described in
`docs/guides/release.md` only, and referenced — not restated — from script
comments.

**Out of scope: editorial guidance.** Document the *mechanism* — where the
summary lives, what the marker separates, that the release cannot be merged
until the summary is written. Do not document what to write or how to word it:
audience, voice, and structure are deliberately out of scope for this
repository. The same applies to `summary_placeholder`, whose text
states where the summary goes and what it becomes, and nothing about style.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `.github/scripts/release-summary.sh` defines `changelog_marker`,
      `summary_region`, `summary_placeholder`, and `summary_is_unwritten`, and
      stays free of `gh` so its functions are testable without the network:
      `! grep -qE '(^|[^[:alnum:]_])gh ' .github/scripts/release-summary.sh`.
      The `gh`-dependent PR lookup lives in `release-pr-lookup.sh` instead.
- [x] `bash .github/scripts/release-summary.test.sh` passes and covers all six
      cases listed in step 5 above (it is the second term of `check_command`).
- [x] The marker string lives in `release-summary.sh` and nowhere else — every
      other script and workflow obtains it from `changelog_marker`. Assert both
      halves, so the gate cannot pass by the marker simply not existing:
      `grep -q 'changelog:auto' .github/scripts/release-summary.sh && [ -z "$(grep -rl 'changelog:auto' .github/ | grep -v 'release-summary')" ]`.
      (The test script is exempt on purpose: asserting the split against a
      literal marker is what makes it an independent check rather than a
      tautology.)
- [x] `find_release_pr` is defined exactly once across `.github/scripts/`, and
      that definition is in `release-pr-lookup.sh`:
      `grep -q '^find_release_pr()' .github/scripts/release-pr-lookup.sh && [ "$(grep -rc '^find_release_pr()' .github/scripts/ | awk -F: '{s+=$2} END{print s+0}')" -eq 1 ]`.
- [x] `release.yml` no longer invokes `generate-changelog.sh`, and
      `create-release-pr.sh` still does:
      `! grep -q generate-changelog .github/workflows/release.yml && grep -q generate-changelog .github/scripts/create-release-pr.sh`.
- [x] The summary-resolution step in `release.yml` appears before the
      `Create GitHub release` step in file order, so a missing summary aborts
      the run before a tag is cut (verified by a grep-based line-number
      comparison appended to `check_command`, or by inspecting the diff).
- [x] `validate-release-pr.yml` runs `validate-release-summary.sh` under the
      same `steps.gate.outputs.skip != 'true'` condition that guards the
      existing title validation.
- [x] Every changed shell script passes `bash -n`.
- [x] `docs/guides/release.md` documents the summary region, the marker, and
      the pre-merge gate; `grep -q 'changelog:auto' docs/guides/release.md`.
- [x] Neither `docs/guides/release.md` nor `summary_placeholder` prescribes a
      voice, audience, or section structure for the summary — the mechanism is
      documented, the editorial policy is not (verified by reading the diff).

### Manual / on-hardware (verified by a human before merge)

- [ ] On the open release PR, after this change first regenerates the body: the
      summary placeholder is present above the marker, the changelog below it
      is unchanged in content, and `Validate Release PR` reports red until the
      summary is written and green after.
- [ ] The published GitHub Release for the next tag renders the hand-written
      summary, the link to the release PR, and the compare link — and carries
      no per-commit list.
