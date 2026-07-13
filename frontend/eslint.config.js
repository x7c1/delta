import js from '@eslint/js';
import tseslint from 'typescript-eslint';

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
);
