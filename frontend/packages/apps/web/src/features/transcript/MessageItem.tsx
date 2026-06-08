import { useCallback } from 'react';
import ReactMarkdown from 'react-markdown';
import type { Message } from '@delta/model';
import { Badge, Collapsible } from '@delta/ui-kit';
import { formatLocalDateTime } from '../../utils/formatLocalDateTime';
import { blockSummary, stringifyContent } from './blockSummary';

export interface MessageItemProps {
  message: Message;
  /** Called when the user selects a text range within this message. */
  onSelectQuote?: (message: Message, quote: string) => void;
}

/**
 * Renders a single transcript message. The sender is conveyed by shape alone —
 * no role label: a user turn is a right-aligned rounded bubble with a tinted
 * background, while an assistant turn is plain full-width text on the canvas.
 *
 * Assistant text is Markdown-rendered; user text is rendered verbatim so
 * newlines and any Markdown-like characters the user typed are preserved as
 * plain text. `thinking` and tool blocks are collapsed by default with a
 * one-line summary. Selecting a text range emits the quote for branching.
 */
export function MessageItem({ message, onSelectQuote }: MessageItemProps) {
  const handleMouseUp = useCallback(() => {
    if (!onSelectQuote) {
      return;
    }
    const selection = window.getSelection();
    const quote = selection?.toString().trim();
    if (quote) {
      onSelectQuote(message, quote);
    }
  }, [message, onSelectQuote]);

  const timestamp = formatLocalDateTime(message.created_at);
  const isUser = message.role === 'user';

  const blocks = (
    <div className="space-y-1.5" onMouseUp={handleMouseUp}>
      {message.content.map((block, index) => {
        switch (block.type) {
          case 'text':
            // User text is what the person typed, not authored Markdown.
            // Render it verbatim with preserved whitespace so single
            // newlines survive and characters like `*` stay literal.
            return message.role === 'user' ? (
              <div key={index} className="whitespace-pre-wrap text-slate-800">
                {block.text}
              </div>
            ) : (
              <div key={index} className="markdown-body text-slate-800">
                <ReactMarkdown>{block.text}</ReactMarkdown>
              </div>
            );
          case 'thinking':
            return (
              <Collapsible key={index} summary={blockSummary(block)}>
                <pre className="whitespace-pre-wrap text-slate-600">
                  {block.thinking}
                </pre>
              </Collapsible>
            );
          case 'tool_use':
            return (
              <Collapsible key={index} summary={blockSummary(block)}>
                <pre className="whitespace-pre-wrap text-slate-700">
                  {stringifyContent(block.input)}
                </pre>
              </Collapsible>
            );
          case 'tool_result':
            return (
              <Collapsible
                key={index}
                summary={
                  <span className="flex items-center gap-1">
                    {blockSummary(block)}
                    {block.is_error && <Badge tone="warning">error</Badge>}
                  </span>
                }
              >
                <pre className="whitespace-pre-wrap text-slate-700">
                  {stringifyContent(block.content)}
                </pre>
              </Collapsible>
            );
          case 'other':
            return (
              <Collapsible key={index} summary={blockSummary(block)}>
                <span className="text-slate-500">
                  Unsupported content block.
                </span>
              </Collapsible>
            );
        }
      })}
    </div>
  );

  // User: a right-aligned tinted bubble that does not span the full width.
  if (isUser) {
    return (
      <article
        className="flex flex-col items-end px-3 py-2 text-sm"
        data-role={message.role}
        data-testid="message-item"
      >
        <div className="max-w-[85%] rounded-2xl bg-slate-100 px-3 py-2 text-slate-800">
          {blocks}
        </div>
        {timestamp && (
          <span className="mt-1 text-xs tabular-nums text-slate-400">
            {timestamp}
          </span>
        )}
      </article>
    );
  }

  // Assistant: plain full-width text on the canvas, no background or label.
  return (
    <article
      className="px-3 py-2 text-sm"
      data-role={message.role}
      data-testid="message-item"
    >
      {blocks}
      {timestamp && (
        <span className="mt-1 block text-xs tabular-nums text-slate-400">
          {timestamp}
        </span>
      )}
    </article>
  );
}
