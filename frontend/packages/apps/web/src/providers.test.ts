import { describe, expect, it } from 'vitest';
import { PROVIDER_WIRE_DEFAULT, type AgentProvider } from '@delta/wire-gen';
import {
  AGENT_PROVIDERS,
  DEFAULT_PROVIDER,
  PROVIDER_METADATA,
  PROVIDER_OPTIONS,
} from './providers';

// Compile-time exhaustiveness: `PROVIDER_METADATA` must cover the full wire
// union in both directions. `satisfies Record<AgentProvider, …>` in the module
// already rejects a missing or unknown key at its declaration site; this
// assertion restates the "no missing key" direction here so the test file
// fails to typecheck too if the guarantee is ever weakened (e.g. the record
// is retyped with a partial index signature).
const _exhaustive: Record<AgentProvider, unknown> = PROVIDER_METADATA;
void _exhaustive;

describe('provider metadata', () => {
  it('enumerates every provider in display order', () => {
    expect(AGENT_PROVIDERS).toEqual(['claude', 'codex']);
    expect(AGENT_PROVIDERS).toEqual(Object.keys(PROVIDER_METADATA));
  });

  it('derives picker options from the metadata record, in the same order', () => {
    expect(PROVIDER_OPTIONS).toEqual(
      AGENT_PROVIDERS.map((value) => ({
        value,
        ...PROVIDER_METADATA[value],
      })),
    );
  });

  it('keeps the display names stable', () => {
    // The exact strings are user-visible in the new-session selector and the
    // Settings pickers; a rename here is a product decision, not a refactor.
    expect(PROVIDER_METADATA.claude).toEqual({
      label: 'Claude Code',
      hint: 'Anthropic Claude Code CLI',
    });
    expect(PROVIDER_METADATA.codex).toEqual({
      label: 'Codex',
      hint: 'OpenAI Codex CLI',
    });
  });

  it('defaults the app to Claude', () => {
    expect(DEFAULT_PROVIDER).toBe('claude');
  });

  it('coincides with the wire omit default today', () => {
    // The app default (a product choice) and the backend's omitted-`provider`
    // default (a fixed wire contract) are separate constants that happen to
    // agree. This pins the current coincidence so a change to either is a
    // conscious one.
    expect(DEFAULT_PROVIDER).toBe(PROVIDER_WIRE_DEFAULT);
  });
});
