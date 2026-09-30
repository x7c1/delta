import type { AgentProvider } from '@delta/wire-gen';

/**
 * The names the mock server treats as repeatable for Claude; every other Claude
 * flag is single-valued. A full copy of the backend's `REPEATABLE_FLAGS`
 * (`backend/crates/gateway/claude-agent/src/launch_option_cardinality.rs`): it
 * is a closed list of plain flag names, so unlike the danger predicate's
 * spellings there is nothing the mock has to leave out.
 * `launchOptionChoiceGroup.test.ts` fails when the two lists drift apart.
 */
export const CLAUDE_REPEATABLE_FLAGS: readonly string[] = [
  '--add-dir',
  '--allowedTools',
  '--allowed-tools',
  '--disallowedTools',
  '--disallowed-tools',
  '--betas',
  '--file',
  '--mcp-config',
  '--tools',
  '--plugin-dir',
  '--plugin-url',
];

/**
 * The exclusive choice group the mock server puts a launch option in — the
 * option's `name` when the provider reads that name as single-valued, `null`
 * when it may repeat.
 *
 * The real `choice_group` is derived server-side from each provider's own
 * vocabulary, so the browser never computes it; this is the *mock's* copy of
 * that server behavior, needed so a row registered through the mock joins its
 * group the way it would against a real backend (a new `--model` row lands in
 * the `--model` radio group). Claude: every flag but a closed repeatable list;
 * Codex: every field but `config`.
 */
export function launchOptionChoiceGroup(
  provider: AgentProvider,
  name: string,
): string | null {
  if (provider === 'claude') {
    return CLAUDE_REPEATABLE_FLAGS.includes(name) ? null : name;
  }
  return name === 'config' ? null : name;
}
