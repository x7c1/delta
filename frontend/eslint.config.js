import js from '@eslint/js';
import tseslint from 'typescript-eslint';

// Raw-HTML sinks are banned repo-wide. Delta renders agent- and user-authored
// Markdown, and it does so through `react-markdown` *without* `rehype-raw`, so
// no rendering path today turns a message into markup. These selectors are the
// guard that keeps it that way: reaching for one of the sinks below is how that
// property would quietly be lost. They are plain `no-restricted-syntax`
// selectors on purpose — the guard is not worth a new lint dependency.
const DANGEROUS_JSX_PROP = {
  selector: 'JSXAttribute[name.name="dangerouslySetInnerHTML"]',
  message:
    'dangerouslySetInnerHTML renders unsanitized markup. Render text, or an explicitly sanitized element tree.',
};

// The same prop reached through an object — a spread (`<div {...props} />`) or a
// props object assembled elsewhere — which the JSX selector above cannot see.
const DANGEROUS_JSX_PROP_IN_OBJECT = {
  selector: 'Property[key.name="dangerouslySetInnerHTML"]',
  message:
    'dangerouslySetInnerHTML renders unsanitized markup, spread form included. Render text, or an explicitly sanitized element tree.',
};

const INNER_HTML_SINK = {
  selector: 'MemberExpression[property.name="innerHTML"]',
  message:
    'Assigning innerHTML parses a string as markup. Use textContent, or build nodes with the DOM API.',
};

export default tseslint.config(
  {
    ignores: [
      '**/dist/**',
      '**/dist-types/**',
      '**/node_modules/**',
      // Gitignored scratch space (agent logs, repro scripts, etc.); never linted.
      '**/.tmp/**',
      '**/*.config.{js,ts}',
      '**/.dependency-cruiser.cjs',
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    rules: {
      'no-restricted-syntax': [
        'error',
        DANGEROUS_JSX_PROP,
        DANGEROUS_JSX_PROP_IN_OBJECT,
        INNER_HTML_SINK,
      ],
    },
  },
  {
    // Tests build DOM fixtures out of literal HTML they wrote themselves, which
    // is not the untrusted-content path this guard is about. `no-restricted-syntax`
    // replaces rather than extends its options, so the two React sinks are
    // re-listed here — they stay banned in tests too.
    files: ['**/*.test.ts', '**/*.test.tsx'],
    rules: {
      'no-restricted-syntax': ['error', DANGEROUS_JSX_PROP, DANGEROUS_JSX_PROP_IN_OBJECT],
    },
  },
);
