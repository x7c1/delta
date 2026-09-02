---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -q "remarkTrimAutolinkPunctuation" frontend/packages/apps/web/src/features/transcript/AssistantMarkdown.tsx && grep -q "\"unist-util-visit\"" frontend/packages/apps/web/package.json && grep -q "\"@types/mdast\"" frontend/packages/apps/web/package.json && (cd frontend && pnpm install --frozen-lockfile --offline >/dev/null)'
assignee: null
branch: task/0902-1542-fix-trim-cjk-punctuation-from-autolinks
created_at: 2026-09-02T15:42:38Z
updated_at: 2026-09-02T16:24:46Z
---

# fix(transcript): keep CJK punctuation out of autolinked URLs

## Overview

Assistant prose in the conversation pane is rendered by `AssistantMarkdown.tsx`
(`frontend/packages/apps/web/src/features/transcript/AssistantMarkdown.tsx`)
with `react-markdown` 9.1 and `remark-gfm` 4.0, and nothing else. GFM's
autolink-literal extension turns a bare `https://…` into a link by reading
up to the next whitespace and then trimming only **ASCII** trailing
punctuation (`.` `,` `:` `;` `?` `!` and an unbalanced `)`). Full-width /
CJK punctuation is not trimmed, so a URL the agent writes in Japanese prose
swallows whatever follows it. Verified against the installed parser:

| markdown | resulting `href` |
| --- | --- |
| `completed（PR: https://github.com/x7c1/delta/pull/375）。` | `https://github.com/x7c1/delta/pull/375）。` |
| `see https://github.com/x7c1/delta/pull/375。次へ` | `https://github.com/x7c1/delta/pull/375。次へ` |
| `「https://example.com/a?b=1」です` | `https://example.com/a?b=1」です` |
| `see (https://github.com/x7c1/delta/pull/375).` | `https://github.com/x7c1/delta/pull/375` (ASCII trimmed correctly) |
| `<https://github.com/x7c1/delta/pull/375>）。` | correct (explicit autolink is unaffected) |

The link renders underlined but points at a URL that does not exist, so it
cannot be clicked to any effect. This is the GFM-specified behaviour
(GitHub renders the same way), so it will not change upstream; Delta
corrects it on its own side, after parsing.

### Design

1. **A remark plugin that trims the tail of autolink literals.** Add
   `frontend/packages/apps/web/src/features/transcript/remarkTrimAutolinkPunctuation.ts`
   exporting a unified plugin (`() => (tree: Root) => void`). It visits
   every `link` node with `unist-util-visit` and touches only the
   autolink-literal shape — a `link` whose children are exactly one `text`
   node whose `value` equals `node.url` (an explicit `[label](url)` has a
   different text, and is left alone: the author chose the URL). For each
   such node it computes the trimmed URL (rule below); if anything was
   trimmed it sets `node.url` and the child's `value` to the trimmed URL
   and inserts a new `text` node carrying the removed suffix into the
   parent's `children` right after the link, then returns
   `[SKIP, index + 2]` so the visitor continues after the inserted node.
   Types come from `@types/mdast` (`Root`, `Link`, `Text`, `Parent`).
2. **The trimming rule.** Work on the URL string only:
   - **Terminators.** The characters `。、，．：；！？` never occur unencoded
     inside a real URL. Cut the URL at the first occurrence of any of them;
     everything from that character on is the suffix.
   - **Closing brackets and quotes.** Then strip trailing characters from
     the set `）」』】〉》〕｝］` one at a time, but only while the closer is
     *unbalanced* — i.e. the remaining URL contains fewer of its opening
     counterpart (`（「『【〈《〔｛［` respectively) than closers. A balanced
     `（…）` inside the URL stays, so an IRI such as
     `https://ja.wikipedia.org/wiki/デルタ（曖昧さ回避）` keeps its parens.
     Mirror the pairs in one table rather than two parallel strings.
   - Nothing else is trimmed. In particular, non-punctuation CJK text glued
     to a URL (`https://…/375です`) is indistinguishable from an IRI path
     (`https://ja.wikipedia.org/wiki/東京`) and is left as-is — name this as
     a known limit in the module doc.
   - Put the rule in a small exported pure function
     (`trimAutolinkPunctuation(url): { url, suffix }`) so it can be tested
     without a parser, and keep the mdast plumbing separate.
3. **Wire it in.** `AssistantMarkdown.tsx` passes
   `remarkPlugins={[remarkGfm, remarkTrimAutolinkPunctuation]}` — after
   `remarkGfm`, since it post-processes the links GFM produced. Update the
   component's doc comment (it says GFM "enables … autolinks") to mention
   that the tail trimming corrects autolinks for CJK punctuation. Because
   both the persisted transcript message and the live streaming bubble
   render through `AssistantMarkdown`, one wiring covers both.
4. **Dependencies.** `unist-util-visit` (runtime; the same 5.1.0
   `react-markdown` already imports, so no new code enters the bundle) goes
   into `dependencies`, and `@types/mdast` (types only, 4.0.4 already in
   the store) into `devDependencies` of
   `frontend/packages/apps/web/package.json`. Run `pnpm install` from
   `frontend/` so `pnpm-lock.yaml` records the new importer edges (the
   store already holds both versions — expect a lockfile-only change) and
   include the lockfile in the change. CI installs with
   `--frozen-lockfile`, and the `check_command` re-runs
   `pnpm install --frozen-lockfile --offline` to prove the lockfile is
   consistent. dependency-cruiser (`make lint`) must stay green — the
   plugin is a leaf module inside `features/transcript/`.
5. **Tests.**
   - `remarkTrimAutolinkPunctuation.test.ts` — unit tests on the pure
     function: each row of the table above; `。` mid-URL cuts everything
     after it; unbalanced `）` / `」` stripped; balanced `（…）` kept; a URL
     ending in ASCII `)` untouched (GFM already handled it); a clean URL
     returns an empty suffix; `…/375です` untouched.
   - `AssistantMarkdown.test.tsx` (new, next to the component) — render
     through React Testing Library and assert the anchor's `href` and text
     for `completed（PR: https://github.com/x7c1/delta/pull/375）。`
     (anchor `href` and text are the bare URL, and `）。` follows as plain
     text), that an explicit `[PR](https://example.com/x）。)` keeps the
     author's URL, that a URL inside inline code or a fenced block is not
     linked at all, and that `<https://example.com/x>）。` is unchanged.

### Session-state coverage

Not applicable: this is a rendering rule for assistant prose and adds no
operation against a session.

### Pipeline notes

- Frontend-only change; run `make lint` before finishing the work phase.
- The grep / lockfile gates appended to `check_command` fail on `main`
  (the plugin name and both dependencies are absent), so they are real
  gates.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `AssistantMarkdown` renders
      `completed（PR: https://github.com/x7c1/delta/pull/375）。` with an
      anchor whose `href` and text are exactly
      `https://github.com/x7c1/delta/pull/375`, followed by `）。` as plain
      text (vitest, `AssistantMarkdown.test.tsx`).
- [x] The pure trimming function cuts at the first of `。、，．：；！？`,
      strips unbalanced trailing `）」』】〉》〕｝］`, keeps a balanced
      `（…）`, leaves ASCII-terminated and clean URLs untouched, and leaves
      `…/375です` untouched (vitest,
      `remarkTrimAutolinkPunctuation.test.ts`).
- [x] Explicit `[label](url)` links, `<url>` autolinks, and URLs inside
      inline code or fenced code blocks are not altered (vitest,
      `AssistantMarkdown.test.tsx`).
- [x] `AssistantMarkdown.tsx` registers `remarkTrimAutolinkPunctuation`
      after `remarkGfm` (grep gate in `check_command`).
- [x] `unist-util-visit` and `@types/mdast` are declared in the web
      package's `package.json` and `pnpm-lock.yaml` is consistent with them
      (`pnpm install --frozen-lockfile --offline` gate in `check_command`).

## Out of scope

- Repairing malformed explicit links such as `[PR](https://…/375）。` (a
  missing `)`): the URL inside becomes clickable via the autolink path, but
  the stray `[PR](` text stays.
- Trimming non-punctuation CJK text glued to a URL (`…/375です`) — not
  distinguishable from an IRI path.
- Changing how links open (target, rel) or their styling.
- Applying the correction anywhere other than assistant prose rendered by
  `AssistantMarkdown` (user text is rendered verbatim, not as Markdown).
