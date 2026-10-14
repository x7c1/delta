import type { ApiClient } from '@delta/api-client';
import type { NewSessionLaunch } from '../../store/liveStore';
import { useLaunchTrustStore } from './launchTrustStore';

/**
 * What to do with a new-session launch once the launch-trust check has run:
 * send it (carrying the commit the user confirmed, when they were asked), or
 * send nothing because the user cancelled.
 */
export type LaunchTrustDecision =
  | { proceed: true; confirmedCommit: string | null }
  | { proceed: false };

/**
 * Ask the server whether `launch` needs the user's confirmation before Claude
 * Code's trust is pre-accepted for its worktree, and when it does, ask the
 * user in the launch-trust dialog.
 *
 * Only a launch with a worktree is checked: Delta pre-accepts trust for
 * nothing else, so there is nothing to confirm. When the check itself fails
 * the launch goes ahead unconfirmed — the server then refuses to pre-accept
 * anything that needed a confirmation, and the launch fails with a reason the
 * user can see, which is better than a send that silently never happens.
 */
export async function confirmLaunchTrust(
  client: ApiClient,
  launch: NewSessionLaunch,
): Promise<LaunchTrustDecision> {
  const { worktree, workdir, provider } = launch;
  if (worktree === null || workdir === null) {
    return { proceed: true, confirmedCommit: null };
  }
  let check;
  try {
    check = await client.checkLaunchTrust({
      workdir,
      start_point: worktree.start_point,
      provider,
    });
  } catch {
    return { proceed: true, confirmedCommit: null };
  }
  if (!check.needs_confirmation) {
    return { proceed: true, confirmedCommit: null };
  }
  const accepted = await useLaunchTrustStore.getState().ask(check);
  return accepted
    ? { proceed: true, confirmedCommit: check.commit }
    : { proceed: false };
}
