import { useMemo } from 'react';
import {
  threadAncestry,
  type Message,
  type Thread,
} from '@delta/model';
import { useThreadMessagesQuery } from '@delta/api-client';
import { Badge, Breadcrumb, Chip, Panel } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';
import { useComposerStore } from '../../store/composerStore';
import { useLiveStore } from '../../store/liveStore';
import { Composer } from '../composer/Composer';
import { PendingQueue } from '../composer/PendingQueue';
import { MessageItem } from './MessageItem';
import { childThreadsByMessage } from './branches';

export interface TranscriptPaneProps {
  threads: Thread[];
  activeThread: Thread;
}

/**
 * The right pane: the active thread's trunk as a linear list, with a breadcrumb
 * for the current location, branch chips where children sprout, the external
 * input marker, the optimistic pending queue, and the composer.
 */
export function TranscriptPane({ threads, activeThread }: TranscriptPaneProps) {
  const client = useApiClient();
  const setActiveThread = useNavStore((state) => state.setActiveThread);
  const setBranchOrigin = useComposerStore((state) => state.setBranchOrigin);
  const externalInput = useLiveStore((state) => state.externalInput);

  const messagesQuery = useThreadMessagesQuery(client, activeThread.id);
  const messages: Message[] = messagesQuery.data?.messages ?? [];

  const ancestry = useMemo(
    () => threadAncestry(threads, activeThread.id),
    [threads, activeThread.id],
  );
  const childMap = useMemo(
    () => childThreadsByMessage(threads, activeThread.id),
    [threads, activeThread.id],
  );

  const breadcrumbItems = ancestry.map((thread) => ({
    key: thread.id,
    label: thread.title,
    onClick: () => setActiveThread(thread.id),
  }));

  const showExternalInput =
    externalInput !== null && externalInput.threadId === activeThread.id;

  return (
    <Panel
      header={<Breadcrumb items={breadcrumbItems} />}
      footer={<Composer activeThread={activeThread} />}
    >
      {showExternalInput && (
        <div className="flex items-start gap-2 border-b border-slate-100 bg-sky-50 px-3 py-2 text-xs">
          <Badge tone="info">external input</Badge>
          <span className="text-slate-700">{externalInput.prompt}</span>
        </div>
      )}

      <PendingQueue threadId={activeThread.id} />

      {messagesQuery.isLoading && (
        <p className="px-3 py-4 text-sm text-slate-400">Loading transcript…</p>
      )}

      {!messagesQuery.isLoading && messages.length === 0 && (
        <p className="px-3 py-4 text-sm text-slate-400">
          No messages yet. Send the first message below.
        </p>
      )}

      {messages.map((message) => {
        const children = childMap.get(message.uuid) ?? [];
        return (
          <div key={message.uuid}>
            <MessageItem
              message={message}
              onSelectQuote={(msg, quote) =>
                setBranchOrigin({
                  parentThreadId: activeThread.id,
                  semanticParentUuid: msg.uuid,
                  locatorQuote: quote,
                })
              }
            />
            {children.length > 0 && (
              <div className="flex flex-wrap gap-1.5 px-3 pb-2">
                {children.map((child) => (
                  <Chip
                    key={child.id}
                    onClick={() => setActiveThread(child.id)}
                  >
                    ⤷ {child.title} ({children.length})
                    <span className="font-medium">[enter →]</span>
                  </Chip>
                ))}
              </div>
            )}
          </div>
        );
      })}
    </Panel>
  );
}
