import type { Thread } from '@delta/wire-gen';
import { Badge, Button } from '@delta/ui-kit';
import { useComposerStore } from '../../store/composerStore';
import { noticeOf, useLiveStore } from '../../store/liveStore';
import {
  Composer,
  composerDraftKey,
  type ComposerMode,
} from '../composer/Composer';
import { ComposerDraftTargetProvider } from '../composer/composerDraftTarget';
import { ComposerRail } from '../composer/ComposerRail';
import { ContextUsageMeter } from '../composer/ContextUsageMeter';
import { PromptTemplateButton } from '../composer/PromptTemplateButton';
import { ProviderTabs } from '../composer/ProviderTabs';
import { ProviderUnavailableNotice } from '../composer/ProviderUnavailableNotice';
import { PendingQueue } from '../composer/PendingQueue';
import type { PendingEntry } from '../composer/usePendingSends';
import { WorkdirChip } from '../composer/WorkdirDialog';
import { WorktreeOptions } from '../composer/WorktreeOptions';
import { LaunchOptionsPicker } from '../composer/LaunchOptionsPicker';
import { FLOATING_CARD_CLASS } from './floatingCard';

interface TranscriptComposerStackProps {
  /** True for the new-session composer state. */
  newSession: boolean;
  /** The active thread, or null. */
  activeThread: Thread | null;
  /** True when the focused session is closed. */
  readOnly: boolean;
  /** True while the focused session is still starting. */
  spawning: boolean;
  /** True when the focused session's pane lost hook contact. */
  hooksUnreachable: boolean;
  /** Why Delta closed the focused session on its own, if it did. */
  closedReason: string | null;
  /** What the composer sends to. */
  composerMode: ComposerMode;
  /** The pending-send rows shown above the composer. */
  pendingEntries: PendingEntry[];
}

/**
 * The bottom overlay's composer stack: the notices card (closed, lost contact,
 * external input, parked send, pending strip) above the composer card and its
 * rail.
 */
export function TranscriptComposerStack({
  newSession,
  activeThread,
  readOnly,
  spawning,
  hooksUnreachable,
  closedReason,
  composerMode,
  pendingEntries,
}: TranscriptComposerStackProps) {
  const openWorkdirDialog = useComposerStore(
    (state) => state.openWorkdirDialog,
  );
  // The focused session's external-input notice, if any. Keyed per session like
  // the permission notice; visibility is further gated to the active thread below.
  const externalInput = useLiveStore((state) =>
    activeThread
      ? noticeOf(state.notices, activeThread.session_id, 'external_input')
      : null,
  );
  // The focused session's parked-send notice, if any: a composed message the
  // server stopped trying to deliver. No thread gate — the message never
  // reached the conversation, so there is no thread it belongs to.
  const sendParked = useLiveStore((state) =>
    activeThread
      ? noticeOf(state.notices, activeThread.session_id, 'send_parked')
      : null,
  );
  // The focused session's latest context-window usage (a `status_updated`
  // snapshot's `used_percentage`, replace-latest in the store). Drives the
  // meter at the rail's right end. `undefined` when no snapshot has arrived yet
  // (or after a `/compact` cleared it), in which case the meter is omitted
  // rather than shown at 0%. Forwarded straight through; never recomputed here.
  const contextUsage = useLiveStore((state) =>
    activeThread ? state.contextUsage[activeThread.session_id] : undefined,
  );
  const dismissExternalInput = useLiveStore(
    (state) => state.dismissExternalInput,
  );
  const dismissSendParked = useLiveStore((state) => state.dismissSendParked);

  const showExternalInput =
    !newSession &&
    activeThread !== null &&
    externalInput !== null &&
    externalInput.threadId === activeThread.id;

  const showSendParked =
    !newSession && activeThread !== null && sendParked !== null;

  // A starting session is not open, so it arrives here `readOnly` — but it was
  // never closed, and nothing resumes it: a send composed now is accepted and
  // parked as a `queued` row until its launch registers. The composer's own
  // "starting" placeholder is what says so; the closed notice would be a
  // second, wrong explanation.
  const showClosedNotice = readOnly && !newSession && !spawning;
  // Only an open session has hooks to lose: once it is closed, the closed
  // notice (and the resume a send performs) is the whole story.
  const showHooksUnreachable = hooksUnreachable && !readOnly && !newSession;
  // Whether the upper (notices) card has anything to show. Each of these
  // conditions matches exactly one child it gates — `showClosedNotice` the
  // closed notice, `showExternalInput` the external-input notice, and a non-empty
  // `pendingEntries` the pending strip (`PendingQueue` itself renders null when
  // empty) — so the card is omitted entirely rather than rendering an empty
  // box when none of them are present.
  const hasNotices =
    showClosedNotice ||
    showHooksUnreachable ||
    (showExternalInput && activeThread !== null) ||
    (showSendParked && activeThread !== null) ||
    pendingEntries.length > 0;
  return (
    <>
      {/* Upper card: status notices + the pending-send strip, kept visually
          separate from the composer so the (now borderless) textarea has a
          clean boundary of its own. The question and permission cards are NOT
          in this stack: they render inline at the conversation tail in the
          scrolling body (see `TranscriptTailCards`), so they
          follow the conversation in the flow instead of floating over it. */}
      {hasNotices && (
        <div
          className={`${FLOATING_CARD_CLASS} space-y-2 px-3 py-2`}
          data-testid="bottom-notices"
        >
          {showClosedNotice && (
            <div
              className="flex items-center gap-2 rounded border border-border-default bg-surface-elevated px-2 py-1 text-caption text-fg-subtle"
              data-testid="readonly-notice"
            >
              <Badge tone="neutral">closed</Badge>
              <span>
                {closedReason !== null
                  ? `${closedReason}. Sending a message resumes it.`
                  : 'This session is closed. Sending a message resumes it.'}
              </span>
            </div>
          )}

          {showHooksUnreachable && (
            <div
              className="flex items-center gap-2 rounded border border-danger/30 bg-danger/10 px-2 py-1 text-caption text-danger"
              data-testid="hooks-unreachable-notice"
              role="alert"
            >
              <Badge className="shrink-0" tone="warning">
                lost contact
              </Badge>
              <span>
                Delta lost contact with this session. Use the terminal, or
                choose Close in the session&apos;s menu and send again to
                resume.
              </span>
            </div>
          )}

          {showExternalInput && activeThread && (
            <div
              className="flex items-start gap-2 rounded border border-info/30 bg-info/10 px-2 py-1 text-caption"
              data-testid="external-input-notice"
            >
              <Badge className="shrink-0" tone="info">
                external input
              </Badge>
              <span className="min-w-0 flex-1 line-clamp-2 break-words text-fg-muted">
                {externalInput.prompt}
              </span>
              <Button
                className="shrink-0"
                size="sm"
                variant="ghost"
                onClick={() => dismissExternalInput(activeThread.session_id)}
              >
                Dismiss
              </Button>
            </div>
          )}

          {showSendParked && activeThread && sendParked && (
            <div
              className="flex items-start gap-2 rounded border border-danger/30 bg-danger/10 px-2 py-1 text-caption"
              data-testid="send-parked-notice"
              role="alert"
            >
              <Badge className="shrink-0" tone="warning">
                not delivered
              </Badge>
              {/* The message itself is no longer only here: the server put
                  the send back in the queue, held for an explicit release,
                  so the `PendingQueue` row rendered just below shows the text
                  and carries the Send and Cancel actions. This notice only
                  has to explain WHY it is waiting — hence no copy of the
                  text, and no "copy it and try again". */}
              <span className="min-w-0 flex-1 break-words text-fg-muted">
                A message never reached the session — the terminal swallowed
                it twice, so Delta stopped re-sending it. It is waiting in
                the queue below: send it again once the terminal is ready, or
                cancel it.
              </span>
              <Button
                className="shrink-0"
                size="sm"
                variant="ghost"
                onClick={() => dismissSendParked(activeThread.session_id)}
              >
                Dismiss
              </Button>
            </div>
          )}

          <PendingQueue
            entries={pendingEntries}
            sessionSpawning={spawning}
          />
        </div>
      )}

      {/* The composer card and the rail on its top edge, stacked with no gap
          so the rail's items rest on the card's top border. The pair is ONE
          flex child of the overlay, so the overlay's `gap-2` falls above the
          rail (between the notices card and the rail) and the rail's height
          is measured with the rest of the overlay — see {@link ComposerRail}
          for why it must stay in normal flow.

          Being siblings is also why the pair needs a provider around it: the
          prompt-template button rides the rail but edits the draft inside the
          card, and cannot reach that textarea through props. */}
      <ComposerDraftTargetProvider draftKey={composerDraftKey(composerMode)}>
        <div className="flex flex-col">
          {/* The template button is on the rail in both modes and is what gives
              the rail its height; the provider tabs join it only for a new
              session (the provider is fixed once a session runs). The focused
              session's context-usage meter sits at the right end — only in a
              thread, since a new session has no snapshot. */}
          <ComposerRail
            templateSlot={<PromptTemplateButton />}
            providerTabs={newSession ? <ProviderTabs /> : null}
            endSlot={
              !newSession && contextUsage !== undefined ? (
                <ContextUsageMeter usedPercentage={contextUsage} />
              ) : null
            }
          />
          {/* Composer card: the new-session launch pickers (which parameterize
              the spawn) sit directly above the input they configure. */}
          <div
            className={`${FLOATING_CARD_CLASS} space-y-2 px-3 py-2`}
            data-testid="composer-card"
          >
            {/* Why the provider tabs on the rail are disabled, in the server's
                words. It leads the card's stack, directly under the tabs it
                explains — the tabs themselves have no room for a reason. */}
            {newSession && <ProviderUnavailableNotice />}
            {/* A directory is chosen: show it as a chip with a ✎ to change it
                (the ✎ reopens the picker without resetting the selection). The
                chip renders nothing when no directory is selected, so there is
                no button to (re)open the picker from here — that is done via
                "New". */}
            {newSession && <WorkdirChip onEdit={openWorkdirDialog} />}
            {/* Below the directory chip: when the selected directory is a git
                repo, an opt-in to start the session in a fresh worktree (with a
                start-point choice). Renders nothing for a non-git directory. */}
            {newSession && <WorktreeOptions />}
            {newSession && <LaunchOptionsPicker />}
            <Composer mode={composerMode} />
          </div>
        </div>
      </ComposerDraftTargetProvider>
    </>
  );
}
