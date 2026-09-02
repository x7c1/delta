import { PROVIDER_WIRE_DEFAULT, type SendRequest } from '@delta/wire-gen';
import type { NewSessionLaunch } from '../../store/liveStore';

/**
 * The `POST /api/sends` body for a new-session launch — the single place the
 * new-session request shape is built, shared by the composer's Send and the
 * failed-spawn Retry so the two can never drift apart (they did — see
 * {@link NewSessionLaunch}).
 *
 * Every optional field is attached only when it carries meaning: `workdir` and
 * `launch_option_ids` are omitted when unset/empty so the server applies its
 * defaults, `worktree` and `pull_request_number` are omitted unless the session
 * actually has one, and `provider` is omitted when it equals
 * `PROVIDER_WIRE_DEFAULT` — the backend resolves an omitted `provider` to
 * exactly that, so omitting it keeps a send on the default provider
 * byte-for-byte identical to a pre-provider send.
 */
export function newSessionSendBody(
  text: string,
  {
    workdir,
    launchOptionIds,
    provider,
    worktree,
    pullRequestNumber,
  }: NewSessionLaunch,
): SendRequest {
  return {
    new_session: true,
    text,
    ...(workdir ? { workdir } : {}),
    ...(launchOptionIds.length > 0
      ? { launch_option_ids: launchOptionIds }
      : {}),
    ...(worktree ? { worktree } : {}),
    ...(provider !== PROVIDER_WIRE_DEFAULT ? { provider } : {}),
    ...(pullRequestNumber !== null
      ? { pull_request_number: pullRequestNumber }
      : {}),
  };
}
