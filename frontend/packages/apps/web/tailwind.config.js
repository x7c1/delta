import defaultTheme from 'tailwindcss/defaultTheme';

// ---------------------------------------------------------------------------
// Design tokens
//
// This config is the single definition of the app's font stacks, and it names
// the layout/color tokens whose *values* live as CSS custom properties in
// `src/index.css`. The plain `:root` block carries default fallback values;
// `:root[data-theme="..."]` blocks redefine the same names per theme so
// swapping the attribute on `<html>` swaps every utility's resolved color
// atomically (see useTheme.ts). Utilities defined here resolve through those
// variables, so a later user-facing stylesheet can override a token in one
// place and every consumer — Tailwind utilities and the runtime readers in
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
      // Semantic font-size scale. Values live as CSS custom properties in
      // src/index.css (`--delta-text-*`, which also documents how call sites
      // pick a token by role); each utility here references its variable so the
      // value layer stays in one place — a later user-facing stylesheet can
      // override a token on `:root` and every `text-*` utility (plus xterm, via
      // src/theme.ts reading `--delta-text-terminal`) follows. Each entry pairs
      // the size with an explicit line-height. `terminal` is consumed only by
      // xterm through src/theme.ts, but is exposed here for symmetry.
      fontSize: {
        body: ['var(--delta-text-body)', '1.5rem'],
        secondary: ['var(--delta-text-secondary)', '1.375rem'],
        // Pairs the 14px caption (the 12px era paired 1rem); dense rows that
        // must stay tight carry their own leading-* overrides.
        caption: ['var(--delta-text-caption)', '1.25rem'],
        terminal: ['var(--delta-text-terminal)', '1.25rem'],
      },
      colors: {
        // Theme-fixed semantic color tokens. The RGB triple is identical in
        // light and dark themes (declared three times in src/index.css all
        // the same so the contract stays uniform).
        //
        // `terminal-*` overlay the embedded xterm canvas, which is itself
        // theme-fixed dark — flipping them per theme would render the chrome
        // illegible in light mode. `terminal-bg` is also read at runtime by
        // src/theme.ts and handed to xterm, so the panel chrome and the
        // canvas can never disagree.
        //
        // `highlight-wash` is the landing-flash color used by the
        // thread-timeline jump animation (`@keyframes
        // delta-timeline-jump-highlight-fade` in src/index.css). Held at
        // amber-100 in both themes for now; the dark value can be retuned
        // later in one place if dogfooding asks for it.
        'terminal-bg': 'rgb(var(--delta-color-terminal-bg) / <alpha-value>)',
        'terminal-fg': 'rgb(var(--delta-color-terminal-fg) / <alpha-value>)',
        'terminal-fg-strong':
          'rgb(var(--delta-color-terminal-fg-strong) / <alpha-value>)',
        'terminal-overlay':
          'rgb(var(--delta-color-terminal-overlay) / <alpha-value>)',
        'terminal-overlay-hover':
          'rgb(var(--delta-color-terminal-overlay-hover) / <alpha-value>)',
        'highlight-wash':
          'rgb(var(--delta-color-highlight-wash) / <alpha-value>)',
        // Semantic color tokens. Values come from the active
        // `:root[data-theme="..."]` block in src/index.css; the same names
        // resolve through every theme, so swapping `data-theme` swaps all of
        // these atomically. `border-default` (not `border`) and `accent-fg` /
        // similar dashed names avoid clashing with Tailwind's built-in
        // single-token utilities (`border`, `text-fg` already lands cleanly
        // because `fg` is not a built-in palette).
        //
        // Each variable stores its color as a space-separated `R G B` triple
        // and is wrapped here as `rgb(var(--X) / <alpha-value>)`. The
        // `<alpha-value>` placeholder is what Tailwind substitutes when a
        // slash-opacity utility is requested (`bg-scrim/40`, `bg-info/15`,
        // `bg-accent/10`, …); without it, Tailwind v3 cannot apply opacity
        // modifiers to CSS-variable-based colors and the utility silently
        // emits no CSS. See:
        // https://tailwindcss.com/docs/customizing-colors#using-css-variables
        surface: 'rgb(var(--delta-color-surface) / <alpha-value>)',
        'surface-elevated':
          'rgb(var(--delta-color-surface-elevated) / <alpha-value>)',
        'surface-elevated-hover':
          'rgb(var(--delta-color-surface-elevated-hover) / <alpha-value>)',
        'surface-sunken':
          'rgb(var(--delta-color-surface-sunken) / <alpha-value>)',
        'surface-sunken-hover':
          'rgb(var(--delta-color-surface-sunken-hover) / <alpha-value>)',
        fg: 'rgb(var(--delta-color-fg) / <alpha-value>)',
        'fg-muted': 'rgb(var(--delta-color-fg-muted) / <alpha-value>)',
        'fg-subtle': 'rgb(var(--delta-color-fg-subtle) / <alpha-value>)',
        'border-default': 'rgb(var(--delta-color-border) / <alpha-value>)',
        'border-strong':
          'rgb(var(--delta-color-border-strong) / <alpha-value>)',
        accent: 'rgb(var(--delta-color-accent) / <alpha-value>)',
        'accent-fg': 'rgb(var(--delta-color-accent-fg) / <alpha-value>)',
        'accent-hover': 'rgb(var(--delta-color-accent-hover) / <alpha-value>)',
        'accent-disabled':
          'rgb(var(--delta-color-accent-disabled) / <alpha-value>)',
        danger: 'rgb(var(--delta-color-danger) / <alpha-value>)',
        warning: 'rgb(var(--delta-color-warning) / <alpha-value>)',
        info: 'rgb(var(--delta-color-info) / <alpha-value>)',
        success: 'rgb(var(--delta-color-success) / <alpha-value>)',
        scrim: 'rgb(var(--delta-color-scrim) / <alpha-value>)',
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
        // inset and the scroll-body padding that reserves space for the
        // composer card derive from the same variables, so they cannot drift
        // apart. The breadcrumb used to have its own reserve when it floated
        // as a card; it is now an in-flow element at the top of the body, so
        // no top reserve token is needed.
        'overlay-inset': 'var(--delta-overlay-inset)',
        'composer-reserve': 'var(--delta-composer-body-reserve)',
      },
    },
  },
  plugins: [],
};
