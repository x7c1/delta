import defaultTheme from 'tailwindcss/defaultTheme';

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
        // The mono twin of the `sans` fix above. Tailwind's default `mono`
        // stack is Latin-only and ends in a generic `monospace`, which the
        // browser resolves for CJK to a *proportional* face — drawing
        // punctuation (`、`/`。`) shoved into the left of the cell instead of
        // centered. Reuse the default Latin cascade as-is (no drift on Tailwind
        // upgrades), then slot an explicit *monospaced* CJK face before the
        // generic `monospace`. Preflight resolves `code`/`kbd`/`samp`/`pre`
        // from this key, so this also fixes the conversation pane's `<pre>`
        // blocks and Markdown code. Mirrors the terminal fix in TerminalPane.
        mono: [
          ...defaultTheme.fontFamily.mono.slice(0, -1),
          '"Noto Sans Mono CJK JP"',
          '"Hiragino Sans"',
          'monospace',
        ],
      },
    },
  },
  plugins: [],
};
