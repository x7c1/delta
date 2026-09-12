---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check'
assignee: null
branch: task/0912-0310-fix-end-github-issue-urls-at-the-first-non-ascii-character
created_at: 2026-09-12T03:10:00Z
updated_at: 2026-09-12T04:13:11Z
---

# fix(transcript): end GitHub pull request and issue URLs at the first non-ASCII character

## Overview

`remarkTrimAutolinkPunctuation`
(`frontend/packages/apps/web/src/features/transcript/remarkTrimAutolinkPunctuation.ts`,
added in #376) moves CJK punctuation the GFM autolinker absorbed back into the
prose. It uses two rules that hold for any URL: cut at the first sentence
terminator (`。、，．：；！？`), then strip full-width closing brackets that
have no opener earlier in the URL. Balance is what protects a genuine IRI such
as `https://ja.wikipedia.org/wiki/デルタ（曖昧さ回避）`, whose parentheses are
part of the address.

That balance rule is exactly what leaves the most common case in Delta's own
prose broken. When an agent writes

```
https://github.com/x7c1/delta/pull/375（実機確認済み）
```

the parentheses are balanced, so the whole `（実機確認済み）` stays inside the
href and the link 404s. The same holds for `…/pull/375「WIP」` and for
non-punctuation text glued on, `…/pull/375です`, which the module's doc comment
already names as a known limit — a generic URL cannot tell an IRI path from
prose.

A GitHub pull-request or issue URL can: its path is
`/<owner>/<repo>/pull/<number>` or `/<owner>/<repo>/issues/<number>` followed
only by ASCII (`/files`, `#issuecomment-…`, `?w=1`). No non-ASCII character
can legitimately appear after the number, so for that one shape the URL can be
ended at the first non-ASCII character without weighing balance at all. Those
are the links that dominate assistant prose in this repository.

### Design

1. **A separate pure rule, tried first.** Add a second exported pure function
   beside `trimAutolinkPunctuation` — same `{ url, suffix }` contract, so
   `url + suffix` still reconstructs the input — that recognises the GitHub
   pull-request / issue shape and cuts at the first non-ASCII character after
   the issue number. `trimAutolinkPunctuation` tries it first and falls back
   to the existing generic rules when the URL is not that shape. Keep the two
   rules as separate functions rather than branching inside the character
   loop: they answer different questions and are tested separately.
2. **Recognising the shape.** Match, case-insensitively for the host and
   scheme, an optional `https?://`, an optional `www.`, then `github.com/`,
   one owner segment, one repo segment, then `pull/` or `issues/` and one or
   more digits. Match against the autolink's *text*, not `node.url`: for a
   `www.…` literal GFM prepends a scheme that the text does not carry, and
   the existing plugin already re-attaches that scheme after trimming.
3. **Cutting.** From the end of the matched prefix, scan forward and cut at
   the first character whose code point is above U+007F; everything from
   there on is the suffix. ASCII tails stay attached, so `/files`,
   `#issuecomment-1234`, `?w=1` and a trailing `/` are preserved. When the
   remainder is all ASCII, the suffix is empty and the URL is returned
   unchanged.
4. **Ordering matters, so pin it.** The generic rule would cut
   `…/pull/375。（補足）` at `。` and stop; the GitHub rule cuts at the same
   place. But for `…/pull/375（補足）` only the GitHub rule cuts at all.
   Running GitHub-first and returning its result — rather than running both
   and taking the shorter — keeps one rule in charge per URL and keeps the
   reasoning readable. Add a test that fixes this ordering.
5. **Documentation.** The module doc comment currently states as a known
   limit that non-punctuation text glued to a URL is left alone. That is no
   longer true for GitHub pull-request and issue URLs; rewrite the limit to
   name the exception and why it is safe (no non-ASCII character can follow
   the issue number), so the doc and the code do not contradict each other.
6. **Tests.**
   - `remarkTrimAutolinkPunctuation.test.ts` — on the new pure function:
     `…/pull/375（実機確認済み）` and `…/issues/12「WIP」` cut at the bracket;
     `…/pull/375です` cuts at `で`; `…/pull/375/files`, `…/pull/375#issuecomment-1`
     and `…/pull/375?w=1` are untouched; the `www.github.com/…` form and an
     uppercase `GITHUB.COM` host are both recognised; a non-GitHub URL with
     balanced full-width parens (`https://ja.wikipedia.org/wiki/デルタ（曖昧さ回避）`)
     still keeps them, proving the generic path is unaffected; a GitHub URL
     that is not a pull/issue path (`https://github.com/x7c1/delta/tree/main/デルタ`)
     falls through to the generic rule.
   - `AssistantMarkdown.test.tsx` — end to end through the renderer:
     `詳細は https://github.com/x7c1/delta/pull/375（実機確認済み）。` produces an
     anchor whose `href` and text are exactly the bare PR URL, and
     `（実機確認済み）。` follows as plain text, with the paragraph's full text
     unchanged.

### Session-state coverage

Not applicable: this is a rendering rule for assistant prose and adds no
operation against a session.

### Pipeline notes

- Frontend-only change, no new dependency; run `make lint` before finishing
  the work phase.
- The acceptance criteria are carried by vitest cases that `make check` runs,
  so no extra gate is appended to `check_command`. Each criterion below names
  the test that decides it.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `https://github.com/x7c1/delta/pull/375（実機確認済み）` and
      `https://github.com/x7c1/delta/issues/12「WIP」` trim to the bare URL,
      with the bracketed text returned as the suffix (vitest,
      `remarkTrimAutolinkPunctuation.test.ts`).
- [x] `https://github.com/x7c1/delta/pull/375です` trims to the bare URL —
      the case the generic rules cannot decide (vitest,
      `remarkTrimAutolinkPunctuation.test.ts`).
- [x] ASCII tails survive: `…/pull/375/files`, `…/pull/375#issuecomment-1`
      and `…/pull/375?w=1` are returned unchanged with an empty suffix
      (vitest, `remarkTrimAutolinkPunctuation.test.ts`).
- [x] The shape is recognised through `www.github.com/…` and an uppercase
      host, and is not recognised for a non-pull/issue GitHub path, which
      falls through to the existing generic rules (vitest,
      `remarkTrimAutolinkPunctuation.test.ts`).
- [x] A non-GitHub URL with balanced full-width parentheses keeps them
      (vitest, existing cases in `remarkTrimAutolinkPunctuation.test.ts`
      still pass unmodified).
- [x] Rendering `詳細は https://github.com/x7c1/delta/pull/375（実機確認済み）。`
      through `AssistantMarkdown` yields an anchor whose `href` and text are
      the bare PR URL, followed by `（実機確認済み）。` as plain text, with the
      paragraph's text content unchanged (vitest,
      `AssistantMarkdown.test.tsx`).
- [x] The module doc comment's "known limit" paragraph names the GitHub
      pull-request / issue exception, so it no longer contradicts the code
      (diff inspection).

## Out of scope

- Other GitHub URL shapes (`/discussions/<n>`, `/commit/<sha>`, `/tree/<ref>`,
  `/compare/…`) and other forges. The rule rests on "no non-ASCII character
  can follow the number" and is not extended past the paths where that holds.
- Repairing malformed explicit links such as `[PR](https://…/375）。`.
- Changing how links open or how they are styled.
