import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { CLAUDE_REPEATABLE_FLAGS } from './launchOptionChoiceGroup';

/** src/ -> api-mocks/ -> testing/ -> packages/ -> frontend/ -> repo root */
const CARDINALITY_SOURCE = fileURLToPath(
  new URL(
    '../../../../../backend/crates/gateway/claude-agent/src/launch_option_cardinality.rs',
    import.meta.url,
  ),
);

/** The string literals of the Rust `REPEATABLE_FLAGS` array. */
function backendRepeatableFlags(): string[] {
  const source = readFileSync(CARDINALITY_SOURCE, 'utf8');
  const body = /const REPEATABLE_FLAGS: &\[&str\] = &\[([\s\S]*?)\];/.exec(
    source,
  )?.[1];
  if (body === undefined) {
    throw new Error(`REPEATABLE_FLAGS not found in ${CARDINALITY_SOURCE}`);
  }
  // Drop `//` comments first: they may quote text that is not a flag.
  const code = body.replace(/\/\/.*$/gm, '');
  const flags = [...code.matchAll(/"([^"]+)"/g)].map((m) => m[1]);
  if (flags.length === 0) {
    throw new Error(`REPEATABLE_FLAGS in ${CARDINALITY_SOURCE} parsed as empty`);
  }
  return flags;
}

describe('CLAUDE_REPEATABLE_FLAGS', () => {
  it('holds the same flags as the backend REPEATABLE_FLAGS', () => {
    // A flag added on one side only fails here until the other side follows,
    // so the mock never groups rows differently from the real server.
    expect([...CLAUDE_REPEATABLE_FLAGS].sort()).toEqual(
      backendRepeatableFlags().sort(),
    );
  });
});
