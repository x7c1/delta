import typography from '@tailwindcss/typography';

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
    extend: {},
  },
  // The `prose` classes give rendered assistant Markdown consistent spacing and
  // typography (paragraphs, lists, tables, headings) without hand-rolling each
  // element. See MessageItem, which applies `prose prose-sm prose-slate`.
  plugins: [typography],
};
