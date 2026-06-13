import defaultTheme from 'tailwindcss/defaultTheme';

// ---------------------------------------------------------------------------
// Design tokens
//
// This config is the single definition of the app's font stacks, and it names
// the layout/color tokens whose *values* live as CSS custom properties in
// `src/index.css` (the `:root` block). Utilities defined here resolve through
// those variables, so a later user-facing stylesheet can override a token in
// one place and every consumer — Tailwind utilities and the runtime readers in
// `src/theme.ts` (xterm) alike — follows. See `src/theme.ts` for the token
// overview.
// ---------------------------------------------------------------------------

// Tailwind's default `sans` cascade ends Latin-only (`ui-sans-serif`,
// `system-ui`, then a generic `sans-serif`). On Linux, `sans-serif` with no
// `lang` tag resolves to the Latin `Noto Sans`, which carries *some* CJK
// punctuation but not all — so `）`(U+FF09) and `。`(U+3002) get split across
// `Noto Sans` and the per-glyph CJK fallback, landing with mismatched metrics
// (the `。` looks shoved off after a full-width `）`). Slot an explicit
// proportional CJK face (Linux Noto, macOS Hiragino) *before* the generic
// `sans-serif` so all CJK resolves from one font; Latin still wins on the
// preceding `ui-sans-serif`/`system-ui`, and the emoji faces stay at the tail.
const sansIdx = defaultTheme.fontFamily.sans.indexOf('sans-serif');
const sans = [
  ...defaultTheme.fontFamily.sans.slice(0, sansIdx),
  '"Noto Sans CJK JP"',
  '"Hiragino Sans"',
  ...defaultTheme.fontFamily.sans.slice(sansIdx),
];

// The monospaced CJK tail shared by the `mono` and `terminal` stacks: an
// explicit *monospaced* CJK face before the generic `monospace`, so full-width
// punctuation (`、` U+3001 / `。` U+3002) lands centered in its cell instead of
// resolving to a proportional fallback.
const monoCjk = ['"Noto Sans Mono CJK JP"', '"Hiragino Sans"'];

// The mono twin of the `sans` fix above. Tailwind's default `mono` stack is
// Latin-only and ends in a generic `monospace`, which the browser resolves for
// CJK to a *proportional* face — drawing punctuation (`、`/`。`) shoved into
// the left of the cell instead of centered. Reuse the default Latin cascade
// as-is (no drift on Tailwind upgrades), then slot the explicit CJK tail
// before the generic `monospace`. Preflight resolves `code`/`kbd`/`samp`/
// `pre` from this key, so this also fixes the conversation pane's `<pre>`
// blocks and Markdown code.
const mono = [
  ...defaultTheme.fontFamily.mono.slice(0, -1),
  ...monoCjk,
  'monospace',
];

// The embedded terminal's stack (consumed by xterm via `--delta-font-terminal`
// — see `src/theme.ts`). font-family falls back per character, so the list
// must lead with a *monospaced* Latin face for each OS — otherwise Latin
// glyphs themselves fall through to the CJK face below, and when that face is
// *proportional* (macOS `Hiragino Sans`) the variable-width letters get
// crammed into xterm's fixed cells and the whole grid looks ragged. With a
// real mono face first (macOS `Menlo`, Windows `Consolas`, Linux `DejaVu Sans
// Mono`), Latin stays monospaced and only CJK drops to the explicit CJK tail.
// Deliberately not the `mono` stack above: that one leads with `ui-monospace`/
// `SFMono-Regular`, which xterm's cell-metric measurement was not tuned
// against — this list is the one proven to keep the grid aligned.
const terminal = ['Menlo', 'Consolas', '"DejaVu Sans Mono"', ...monoCjk, 'monospace'];

/** @type {import('tailwindcss').Config} */
export default {
  content: [
    './index.html',
    './src/**/*.{ts,tsx}',
    // Scan ui-kit sources so the utility classes used by shared components are
    // not purged from the build.
    '../../ui/ui-kit/src/**/*.{ts,tsx}',
  ],
  theme: {
    extend: {
      fontFamily: {
        sans,
        mono,
        terminal,
      },
      colors: {
        // The embedded terminal's background (xterm reads the same variable,
        // so the panel chrome and the canvas can never disagree).
        'terminal-bg': 'var(--delta-terminal-bg)',
      },
      keyframes: {
        // A hard on/off blink for the live-streaming caret. The `steps(1, end)`
        // timing (see `animation` below) snaps between these two stops with no
        // tween, so it reads as a text cursor blinking rather than a soft pulse.
        'caret-blink': { '0%,49%': { opacity: '1' }, '50%,100%': { opacity: '0' } },
      },
      animation: {
        'caret-blink': 'caret-blink 1.1s steps(1, end) infinite',
      },
      spacing: {
        // Overlay layout tokens (values in src/index.css). The floating-card
        // insets and the scroll-body paddings that reserve space for those
        // cards derive from the same variables, so they cannot drift apart.
        'overlay-inset': 'var(--delta-overlay-inset)',
        'composer-reserve': 'var(--delta-composer-body-reserve)',
        'breadcrumb-reserve': 'var(--delta-breadcrumb-body-reserve)',
      },
    },
  },
  plugins: [],
};
