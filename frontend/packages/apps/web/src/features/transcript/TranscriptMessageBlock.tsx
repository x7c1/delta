import type { ThreadId } from '@delta/model';
import type { Message, Thread } from '@delta/wire-gen';
import { Chip } from '@delta/ui-kit';
import {
  isTaskNotificationMessage,
  isUnknownCommandNoticeMessage,
} from './claudeFormat';
import { MessageItem } from './MessageItem';
import { messageRendersProse, type ToolPairing } from './toolPairs';

interface TranscriptMessageBlockProps {
  /** The message this block renders. */
  message: Message;
  /** Sub-threads keyed by the message they sprout from. */
  childMap: Map<string, Thread[]>;
  /** Tool calls resolved to their results across the thread. */
  pairing: ToolPairing;
  /** The latest assistant message's uuid; only it shows the richer meta. */
  latestAssistantUuid: string | null;
  /** Arm a branch from a quote selected in the message. */
  handleSelectQuote: (msg: Message, quote: string) => void;
  /** The sub-thread chip flashed after a breadcrumb "go up", if any. */
  flashChildId: ThreadId | null;
  /** Set or clear the hovered sub-thread chip's highlight. */
  setHoveredBranchTitle: (title: string | null) => void;
  /** Enter a sub-thread. */
  setActiveThread: (threadId: ThreadId) => void;
}

/**
 * One message of the transcript with the chips of the sub-threads that sprout
 * from it.
 */
export function TranscriptMessageBlock({
  message,
  childMap,
  pairing,
  latestAssistantUuid,
  handleSelectQuote,
  flashChildId,
  setHoveredBranchTitle,
  setActiveThread,
}: TranscriptMessageBlockProps) {
  const children = childMap.get(message.uuid) ?? [];
  // A nested aside card is tightened and left-indented so it reads as a
  // nested step rather than prose. Two independent things make a message
  // one, and they are kept separate on purpose.
  //
  // Its *shape*: it renders only standalone cards, never prose — an
  // assistant tool call (`tool_use`), an orphan tool result (`tool_result`;
  // paired results are already dropped as empty-rendering, so any that
  // survive are orphans Claude delivers as `role: user`), or the model's
  // reasoning when a provider delivers it as its own message. This is the
  // same `messageRendersProse` question `MessageItem` asks for the prose
  // bubble, so the indent, the tightened gaps and the bubble cannot
  // disagree, and a new card-rendering block kind is covered everywhere at
  // once. It is asked before the user/prose split, so an orphan
  // tool_result is treated as a card rather than a user turn.
  //
  // Its *category*: the harness-injected task-notification card (a
  // collapsed `<task-notification>` user turn), meta lines, and the
  // unknown-command system notice all carry prose yet are still asides —
  // the notice is a system aside, not the agent's reply. These are semantic
  // categories rather than rendering shapes, so they stay explicit terms.
  const isNestedCard =
    !messageRendersProse(message, pairing) ||
    isTaskNotificationMessage(message) ||
    message.role === 'meta' ||
    isUnknownCommandNoticeMessage(message);
  const topGap = isNestedCard
    ? 'pt-0.5'
    : message.role === 'user'
      ? 'pt-2'
      : 'pt-1.5';
  const bottomGap = isNestedCard
    ? 'pb-0.5'
    : message.role === 'user'
      ? 'pb-1'
      : 'pb-2';
  // Inset a nested card (left margin) so it reads as a nested step.
  const indent = isNestedCard ? 'ml-6 mr-0' : '';
  return (
    // One block per message: the message and its sub-thread chips. The
    // block owns the vertical rhythm (not the message article), so adjacent
    // messages are separated by a consistent gap while the chips hug their
    // parent message just below it (see their small pt).
    <div className={`${topGap} ${bottomGap} ${indent}`}>
      <MessageItem
        message={message}
        pairing={pairing}
        isLatest={message.uuid === latestAssistantUuid}
        onSelectQuote={handleSelectQuote}
      />
      {children.length > 0 && (
        <div className="flex flex-wrap justify-end gap-1.5 px-3 pt-1.5">
          {children.map((child) => (
            // The wrapper carries the scroll target id so a breadcrumb
            // "go up" can bring this chip into view (see scrollToChildRef).
            <span
              key={child.id}
              data-child-thread-id={child.id}
              className="inline-flex"
            >
              <Chip
                // The chip's clickable pill shape conveys that it enters
                // the branch, so no "[enter →]" label is shown. The
                // accessible name still says "Enter <title>" for screen
                // readers (and to distinguish it from the navigator tree
                // node of the same branch).
                ariaLabel={`Enter ${child.title}`}
                // Briefly ring the chip after a breadcrumb "go up" scrolls
                // it into view, so the eye catches where the branch began.
                className={
                  flashChildId === child.id
                    ? 'ring-2 ring-accent-hover ring-offset-1'
                    : undefined
                }
                // Clear the hover highlight on click: entering the branch
                // does not fire mouseleave, so the mark would otherwise
                // linger across the whole child thread.
                onClick={() => {
                  setHoveredBranchTitle(null);
                  setActiveThread(child.id);
                }}
                // Hovering the chip marks every occurrence of its text in
                // the body, so it is clear what the branch was about.
                onMouseEnter={() => setHoveredBranchTitle(child.title)}
                onMouseLeave={() => setHoveredBranchTitle(null)}
              >
                ⤷ {child.title}
              </Chip>
            </span>
          ))}
        </div>
      )}
    </div>
  );
}
