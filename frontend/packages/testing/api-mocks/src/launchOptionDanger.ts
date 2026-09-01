import type { AgentProvider } from '@delta/wire-gen';

/**
 * Which launch options the mock server treats as "dangerous" — the ones that
 * switch the agent's own safety mechanism off.
 *
 * The real verdict is derived server-side from each provider's own vocabulary,
 * so the browser never computes it; this is the *mock's* copy of that server
 * behavior, needed for the same reason the mock copies the `409` on a built-in
 * delete: without it the marking, the refused default and the picker's
 * never-pre-check rule could not be driven at all without a backend.
 *
 * Deliberately the headline spellings rather than an exhaustive port of the
 * backend predicate (which reaches into Codex `config` values in both of their
 * spellings): a mock has to be recognisably the same rule, not a second
 * implementation of it. Component tests that need a specific verdict override
 * the list handler with a literal row instead.
 */
export function isDangerousLaunchOption(
  provider: AgentProvider,
  name: string,
  value: string | null,
): boolean {
  if (provider === 'claude') {
    return (
      name === '--dangerously-skip-permissions' ||
      (name === '--permission-mode' && value === 'bypassPermissions')
    );
  }
  return (
    (name === 'sandbox' && value === 'danger-full-access') ||
    (name === 'approvalPolicy' && value === 'never') ||
    (name === 'config' &&
      value !== null &&
      ((value.includes('sandbox_mode') &&
        value.includes('danger-full-access')) ||
        (value.includes('approval_policy') && value.includes('never'))))
  );
}
