import type { SessionListItem } from '@delta/wire-gen';
import {
  ApiError,
  useDeleteSessionMutation,
  useSessionSendsQuery,
} from '@delta/api-client';
import { Badge, Button } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { useNewSessionSend } from '../composer/useNewSessionSend';
import { useLiveStore } from '../../store/liveStore';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { useNotificationStore } from '../../store/notificationStore';

export interface FailedSessionPaneProps {
  /** The failed session's list row — the whole of what is known about it. */
  item: SessionListItem;
}

/**
 * The screen of a session whose launch never bound.
 *
 * There is no conversation to show and no composer: nothing bound, nothing was
 * ingested, and a send here would be asking to resume a session that never
 * started. So the pane renders in the transcript's place rather than as a
 * notice above a composer.
 *
 * Both of the things it shows — `session.failure_reason` and the session's
 * still-open sends — come from the server rather than from this browser's
 * memory, so a failure survives a reload and is readable in a tab that never
 * started the launch. An ending that named no cause (the watchdog-shaped ones)
 * is said to be unexplained rather than left blank.
 *
 * **Retry** re-sends the first prompt as the identical launch and removes this
 * row, so a successful retry leaves one session rather than a live one beside a
 * dead duplicate. Whatever the user typed *behind* that first prompt is removed
 * with the row and is not re-sent, so the pane says as much: this screen is the
 * last place that text can be read. It needs the launch configuration — the
 * launch-option ids and the worktree request, which no row records — and that
 * lives only in this browser's spawn registry, so Retry is offered exactly when
 * this window is the one that started the launch. After a reload, or in another
 * tab, **Remove** is offered alone and the new-session screen is where a fresh
 * launch is configured.
 */
export function FailedSessionPane({ item }: FailedSessionPaneProps) {
  const client = useApiClient();
  const sessionId = item.session.id;
  const sendsQuery = useSessionSendsQuery(client, sessionId);
  const deleteSession = useDeleteSessionMutation(client);
  const retrySpawn = useNewSessionSend();
  const clearSpawn = useLiveStore((state) => state.clearSpawn);
  const setFocusedSession = useNavStore((state) => state.setFocusedSession);
  const showError = useNotificationStore((state) => state.showError);

  // The launch this browser started, kept for its configuration alone (see
  // `SpawnItem.status`). Absent after a reload, and in every other tab.
  const spawn = useLiveStore((state) =>
    state.spawns.find(
      (candidate) =>
        candidate.sessionId === sessionId && candidate.status === 'failed',
    ),
  );

  // Everything the session accepted and never delivered, oldest first — the
  // server's own open-send list, which for a failed session is exactly the set
  // of messages that never reached an agent.
  const undelivered = sendsQuery.data?.sends ?? [];

  // The user asked for this ending: they closed the session while it was still
  // starting. It is not a breakage, so it is worded and toned as the thing that
  // happened. `cancelled` is only known to the window that saw the event; a
  // reload reads the persisted reason, which names the close in its own words.
  const cancelled = spawn?.cancelled === true;

  const remove = (onSuccess?: () => void) =>
    deleteSession.mutate(sessionId, {
      onSuccess: () => {
        clearSpawn(sessionId);
        onSuccess?.();
      },
      onError: (error: unknown) => {
        showError(
          'Could not remove the session',
          error instanceof ApiError || error instanceof Error
            ? error.message
            : 'The request failed.',
        );
      },
    });

  return (
    // Two boxes, not one: the outer is the scroll container and the centring
    // happens inside it, on a `min-h-full` box that simply grows when the card
    // is taller than the pane. Centring on the scroll container itself splits a
    // tall card's overflow evenly above and below, and the half above cannot be
    // scrolled to — which would put the heading and the reason (the one thing
    // the user came here to read) permanently out of reach behind a long
    // undelivered prompt.
    <div className="h-full overflow-y-auto" data-testid="failed-session-pane">
      <div className="flex min-h-full items-center justify-center p-6">
        <div className="w-full max-w-xl space-y-4 rounded-lg border border-border-default bg-surface p-5 shadow-md">
          <div className="flex items-center gap-2">
            <Badge tone={cancelled ? 'neutral' : 'warning'}>
              {cancelled ? 'cancelled' : 'failed'}
            </Badge>
            <h2 className="text-body font-medium text-fg">
              {cancelled
                ? 'This launch was cancelled.'
                : 'This session never started.'}
            </h2>
          </div>

          <p
            className="break-words text-secondary text-fg-muted"
            data-testid="failed-session-reason"
          >
            {item.session.failure_reason ??
              'Delta did not hear why — the launch went quiet before it reported anything.'}
          </p>

          <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-caption text-fg-subtle">
            <dt>Directory</dt>
            <dd className="min-w-0 break-all text-fg-muted">{item.session.cwd}</dd>
            {item.session.repository_display_name !== null && (
              <>
                <dt>Repository</dt>
                <dd className="min-w-0 break-all text-fg-muted">
                  {item.session.repository_display_name}
                </dd>
              </>
            )}
            {item.session.branch_at_launch !== null && (
              <>
                <dt>Branch</dt>
                <dd className="min-w-0 break-all text-fg-muted">
                  {item.session.branch_at_launch}
                </dd>
              </>
            )}
          </dl>

          {undelivered.length > 0 && (
            <div className="space-y-1">
              <p className="text-caption text-fg-subtle">
                {undelivered.length === 1
                  ? 'This message was never delivered:'
                  : 'These messages were never delivered:'}
              </p>
              <ul className="space-y-1">
                {undelivered.map((send) => (
                  <li
                    key={send.id}
                    className="whitespace-pre-wrap break-words rounded border border-border-default bg-surface-elevated px-2 py-1.5 text-caption text-fg"
                    data-testid="failed-session-prompt"
                  >
                    {send.text}
                  </li>
                ))}
              </ul>
              {/* What becomes of the text above, said before the user picks
                  an action rather than discovered afterwards. Every action
                  here deletes the row these sends hang off, and nothing
                  catches them any more — the browser used to append them to
                  the new-session draft — so this screen is the last moment
                  the user can act on them. Retry additionally re-sends the
                  FIRST prompt alone (it is the launch's own, and the only one
                  the tracked spawn holds). Silent in exactly one case: Retry
                  on offer with a single send, which IS that first prompt, so
                  nothing is left behind. */}
              {!(spawn && undelivered.length === 1) && (
                <p
                  className="text-caption text-fg-subtle"
                  data-testid="failed-session-undelivered-fate"
                >
                  {spawn
                    ? 'Retry re-sends the first message only. It also removes this session, so the rest are not kept — copy anything you still need.'
                    : 'Remove deletes this session, and this text with it — copy anything you still need.'}
                </p>
              )}
            </div>
          )}

          <div className="flex justify-end gap-2">
            {spawn && (
              <Button
                size="sm"
                variant="secondary"
                onClick={() => {
                  // Move to the new-session screen first: that is the one place
                  // the workspace hands a fresh spawn's focus over from, so the
                  // retry lands the user in the session it starts (see
                  // `WorkspaceScreen`). Doing it before the POST leaves also
                  // means the removal below is no longer removing the focused
                  // session, so nothing races to reconcile focus a second time.
                  setFocusedSession(NEW_SESSION_FOCUS);
                  retrySpawn({
                    text: spawn.text,
                    workdir: spawn.workdir,
                    launchOptionIds: spawn.launchOptionIds,
                    provider: spawn.provider,
                    worktree: spawn.worktree,
                    pullRequestNumber: spawn.pullRequestNumber,
                  });
                  // The retry supersedes this row; leaving it would show the new
                  // session beside a dead duplicate of itself.
                  remove();
                }}
              >
                Retry
              </Button>
            )}
            <Button size="sm" variant="ghost" onClick={() => remove()}>
              Remove
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
