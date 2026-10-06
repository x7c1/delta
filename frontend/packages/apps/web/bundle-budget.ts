import type { Plugin } from 'vite';

// Fails a production build when any emitted JavaScript chunk exceeds
// `limitKb` (1 kB = 1000 bytes, the unit Vite reports chunk sizes in), instead
// of printing a warning nobody reads. The budget itself and its rationale live
// in `vite.config.ts`. Build only: the dev server serves unbundled modules.
// Sizes are read in `generateBundle`, after Vite has minified each chunk in
// `renderChunk`, and measured in UTF-8 bytes the way Vite's own chunk-size
// warning measures them.
export function bundleBudget(limitKb: number): Plugin {
  return {
    name: 'delta-bundle-budget',
    apply: 'build',
    generateBundle(_options, bundle) {
      const utf8 = new TextEncoder();
      const oversized = Object.values(bundle).flatMap((output) => {
        if (output.type !== 'chunk') {
          return [];
        }
        const sizeKb = utf8.encode(output.code).length / 1000;
        return sizeKb > limitKb ? [`${output.fileName} (${sizeKb.toFixed(2)} kB)`] : [];
      });
      if (oversized.length > 0) {
        this.error(
          `Bundle budget exceeded: ${oversized.join(', ')} > ${limitKb} kB limit. ` +
            'See BUNDLE_BUDGET_KB in frontend/packages/apps/web/vite.config.ts.',
        );
      }
    },
  };
}
