---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && grep -rqF 'pull/229**（検査は通過済み' frontend/packages/apps/web/src/features/transcript/"
assignee: null
branch: task/0929-1340-fix-end-autolinks-before-parsing-so-emphasis-around-them-closes
created_at: 2026-09-29T04:40:27Z
updated_at: 2026-09-29T05:20:41Z
---

# fix(transcript): end autolinks before parsing so emphasis around them closes

## Overview

Assistant prose is rendered by `AssistantMarkdown`
(`frontend/packages/apps/web/src/features/transcript/AssistantMarkdown.tsx`)
with `remark-gfm` followed by `remarkTrimAutolinkPunctuation`
(`remarkTrimAutolinkPunctuation.ts` in the same directory). That plugin runs
*after* parsing: it takes each autolink literal GFM produced and moves the
text the autolinker wrongly absorbed back into the prose, using
`trimGitHubIssueUrl` for GitHub pull-request / issue URLs and the generic
terminator / unbalanced-closer rules otherwise.

An agent routinely writes a URL in bold, directly followed by Japanese prose:

```
PR を作りました: **https://github.com/x7c1/delta/pull/229**（検査は通過済み、CI は実行中です）
```

This still renders wrong. GFM's autolink literal reads the path up to the
next whitespace and only then drops trailing ASCII punctuation (`*`, `_`,
`~`, `.`, …) *when an end follows it* — whitespace, `<` or EOF. Here the
closing `**` is followed by `（`, so GFM links
`https://github.com/x7c1/delta/pull/229**（検査は通過済み、CI`. The post-parse
plugin then cuts at the first non-ASCII character, leaving
`https://github.com/x7c1/delta/pull/229**` as the href. Two defects are
visible:

1. The href ends in `**`, so the link is broken.
2. The closing `**` was consumed by the autolink, so the strong never closes
   and the opening `**` is shown as literal text.

The second one cannot be repaired after parsing: emphasis is resolved while
tokenizing, and by the time the mdast exists the delimiter is gone. The same
happens with generic URLs (`**https://example.com/a**。`) and with `_…_` and
`~~…~~` around a URL.

### Design

Move the cut from after parsing to before it, and let GFM itself end the
link there.

1. **Mark the boundary before parsing.** Scan the Markdown source for
   autolink-literal candidates — a case-insensitive `https?://` or `www.`
   followed by characters up to the next whitespace or `<`, which is the span
   GFM's autolink reads. Run the existing `trimAutolinkPunctuation` on each
   candidate; when its `suffix` is non-empty, insert a sentinel character
   between `url` and `suffix`. `url + suffix` reconstructs the input, so the
   insertion loses nothing. A candidate GFM would not actually link only
   receives a sentinel that step 3 removes again, so the scan may
   over-approximate GFM's start conditions.
2. **The sentinel is U+FEFF.** It is zero-width, and micromark classifies
   Unicode whitespace with JavaScript's `\s`
   (`micromark-util-character`'s `unicodeWhitespace`), which includes U+FEFF.
   GFM therefore sees an end right after the trailing `**`: it drops the `**`
   from the link as trailing punctuation, and because the `**` is now
   right-flanking it closes the strong. That the parser treats U+FEFF as
   whitespace is a dependency on the library's implementation; the tests in
   the acceptance criteria below pin it, so a dependency update that changes
   it fails `make check`.
3. **Strip the sentinel after parsing.** A remark plugin removes every U+FEFF
   from the string fields of every mdast node — `value` (text, inline code,
   code, html) and `url`, `title`, `alt` (links, images, definitions). The
   sentinel can land in a link destination: `[wiki](https://ja.wikipedia.org/wiki/東京。)`
   still parses as a link, but with U+FEFF inside its `url`, so the fields
   other than `value` are not optional. The sentinel must never reach the
   DOM, where it would survive a copy.
4. **Retire the post-parse link rewriting.** With the boundary in place GFM
   produces the right link, so the part of `remarkTrimAutolinkPunctuation`
   that splits link nodes after parsing becomes redundant; remove it rather
   than keeping two mechanisms that must agree. The pure functions
   `trimAutolinkPunctuation` and `trimGitHubIssueUrl` stay and keep their
   existing tests and doc comments (update the doc comments where they
   describe the post-parse plugin). Keep the sentinel private to the module
   that both inserts and strips it; rename the module if its current name no
   longer describes what it does.
5. **Update `AssistantMarkdown`'s doc comment** to describe the pre-parse
   boundary instead of the post-parse trimming.

The cut rules themselves do not change. In particular, an ASCII `*` in the
middle of a URL (`https://web.archive.org/web/*/example.com`) stays in the
link; only a trailing ASCII punctuation run directly before a cut is dropped,
which is what GFM already does when whitespace follows it.

Out of scope: changing where the cut falls for any input. The existing
`AssistantMarkdown` and `trimAutolinkPunctuation` tests describe that
behaviour and must keep passing unchanged.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Rendering
      `PR を作りました: **https://github.com/x7c1/delta/pull/229**（検査は通過済み、CI は実行中です）`
      through `AssistantMarkdown` yields a `<strong>` containing an anchor
      whose `href` is `https://github.com/x7c1/delta/pull/229`, followed by
      the text `（検査は通過済み、CI は実行中です）`, and no literal `**` in
      the rendered text (test in the transcript feature directory; the grep
      appended to `check_command` requires this exact input to be in a test).
- [x] The same holds for a generic URL (`**https://example.com/a**。` renders
      a strong link to `https://example.com/a` followed by `。`), for `_…_`
      (emphasis) and `~~…~~` (strikethrough) around a GitHub pull-request URL
      followed by `（…）`, and for a `www.` pull-request URL in bold (href
      keeps the `http://` GFM prepends).
- [x] An explicit link whose destination contains a cut point,
      `[wiki](https://ja.wikipedia.org/wiki/東京。)`, renders an anchor whose
      `href` is the percent-encoded form of exactly that destination, with no
      U+FEFF (`%EF%BB%BF`) in it.
- [x] A URL inside inline code and inside a fenced code block that the cut
      rules would split (`https://github.com/x7c1/delta/pull/1（補足）`)
      renders character-for-character as written, with no U+FEFF.
- [x] `https://web.archive.org/web/*/example.com です` renders one link whose
      `href` keeps the `*`.
- [x] No rendered text node in any of the above contains U+FEFF.
- [x] All existing tests in `AssistantMarkdown.test.tsx` and the
      `trimAutolinkPunctuation` / `trimGitHubIssueUrl` tests pass unchanged
      in their expectations.

### Manual / on-hardware (verified by a human before merge)

- [ ] In a running Delta, an assistant message containing the bold
      pull-request sentence above shows the URL in bold, the link opens the
      pull request, and copying the sentence from the page yields no hidden
      characters.
