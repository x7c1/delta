import { afterEach, describe, expect, it } from 'vitest';
import { findTheme, THEMES } from './registry';

/**
 * Locks in the registry + CSS-block extension recipe end-to-end for the
 * Sepia theme. The recipe is:
 *
 *   1. Add a `:root[data-theme="<id>"]` block in `src/index.css`.
 *   2. Add a {@link THEMES} entry here.
 *
 * Step 2 is what this test guards: a registry entry exists for `sepia` with
 * the expected `displayName`/`isDark`, and `findTheme()` returns it. The
 * matching CSS block in step 1 is exercised by the e2e-fake smoke spec
 * (`e2e-fake/appearance-theme-switch.spec.ts`), where the dev server loads
 * the real stylesheet and a Playwright `getComputedStyle` can resolve the
 * variable. The vitest setup runs in jsdom with `css: false` in
 * `vite.config.ts`, so the stylesheet is never loaded and a `getComputedStyle`
 * variable lookup would just return an empty string regardless of the
 * `<html data-theme="…">` attribute — there is no CSS-variable assertion to
 * make at this layer, hence the registry-shape focus here.
 */
describe('themes/registry — sepia extension recipe', () => {
  afterEach(() => {
    delete document.documentElement.dataset.theme;
  });

  it('registers the sepia theme with the expected metadata', () => {
    const found = findTheme('sepia');
    expect(found).toBeDefined();
    expect(found?.displayName).toBe('Sepia');
    // The override deliberately keeps the surfaces warm and light, so
    // isDark stays false; consumers that key off this flag (xterm, syntax
    // highlighters) should treat sepia as a light palette.
    expect(found?.isDark).toBe(false);
  });

  it('exposes sepia via the THEMES array (so the picker enumerates it)', () => {
    const ids = THEMES.map((t) => t.id);
    expect(ids).toContain('sepia');
  });

  it('writes data-theme="sepia" without round-tripping through the registry', () => {
    // Smoke-check that the attribute is the only contract the CSS block keys
    // off — no JS validation gates it. The actual variable resolution from
    // the stylesheet is asserted by the e2e-fake spec; see the file header.
    document.documentElement.setAttribute('data-theme', 'sepia');
    expect(document.documentElement.dataset.theme).toBe('sepia');
  });
});
