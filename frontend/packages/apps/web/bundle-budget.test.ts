import { describe, expect, it, vi } from 'vitest';
import type { Rollup } from 'vite';
import { bundleBudget } from './bundle-budget';

// Drives the plugin's `generateBundle` hook directly with a synthetic bundle,
// standing in for Rollup's plugin context with just the `error` it calls.
function runGenerateBundle(limitKb: number, bundle: Record<string, unknown>) {
  const plugin = bundleBudget(limitKb);
  const hook = plugin.generateBundle;
  if (typeof hook !== 'function') {
    throw new Error('bundleBudget must define generateBundle as a function');
  }
  const error = vi.fn((message: string) => {
    throw new Error(message);
  });
  const run = () =>
    hook.call({ error } as never, {} as never, bundle as Rollup.OutputBundle, false);
  return { run, error };
}

function chunk(fileName: string, sizeBytes: number) {
  return { type: 'chunk', fileName, code: 'x'.repeat(sizeBytes) };
}

describe('bundleBudget', () => {
  it('applies to production builds only', () => {
    expect(bundleBudget(1).apply).toBe('build');
  });

  it('fails the build naming each chunk above the limit', () => {
    const { run, error } = runGenerateBundle(100, {
      'assets/index.js': chunk('assets/index.js', 100_001),
      'assets/small.js': chunk('assets/small.js', 10),
    });
    expect(run).toThrow(
      'Bundle budget exceeded: assets/index.js (100.00 kB) > 100 kB limit.',
    );
    expect(error).toHaveBeenCalledOnce();
    expect(error.mock.calls[0][0]).not.toContain('assets/small.js');
  });

  it('passes chunks at the limit and ignores non-chunk assets', () => {
    const { run, error } = runGenerateBundle(100, {
      'assets/index.js': chunk('assets/index.js', 100_000),
      'assets/index.css': {
        type: 'asset',
        fileName: 'assets/index.css',
        source: 'x'.repeat(200_000),
      },
    });
    expect(run).not.toThrow();
    expect(error).not.toHaveBeenCalled();
  });

  it('measures UTF-8 bytes rather than string length', () => {
    // 'é' is one UTF-16 code unit but two UTF-8 bytes.
    const { run } = runGenerateBundle(1, {
      'assets/index.js': { type: 'chunk', fileName: 'assets/index.js', code: 'é'.repeat(501) },
    });
    expect(run).toThrow('assets/index.js (1.00 kB)');
  });
});
