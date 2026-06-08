import { useCallback } from 'react';
import ReactMarkdown from 'react-markdown';
import type { Message } from '@delta/model';
import { Badge, Collapsible, cn } from '@delta/ui-kit';
import { blockSummary, stringifyContent } from './blockSummary';

export interface MessageItemProps {
  message: Message;
  /** Called when the user selects a text range within this message. */
  onSelectQuote?: (message: Message, quote: string) => void;
}

const ROLE_LABEL: Record<Message['role'], string> = {
  user: 'You',
  assistant: 'Assistant',
  system: 'System',
  other: 'Other',
};

/**
 * Renders a single transcript message. Assistant text is foregrounded and
 * Markdown-rendered; user text is rendered verbatim so newlines and any
 * Markdown-like characters the user typed are preserved as plain text.
 * `thinking` and tool blocks are collapsed by default with a one-line summary.
 * Selecting a text range emits the quote for branching.
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

  return (
    <article
      className={cn(
        'border-b border-slate-100 px-3 py-2 text-sm',
        message.role === 'user' && 'bg-slate-50',
      )}
      data-role={message.role}
      data-testid="message-item"
    >
      <div className="mb-1 flex items-center gap-2">
        <span className="text-xs font-semibold text-slate-500">
          {ROLE_LABEL[message.role]}
        </span>
      </div>
      <div className="space-y-1.5" onMouseUp={handleMouseUp}>
        {message.content.map((block, index) => {
          switch (block.type) {
            case 'text':
              // User text is what the person typed, not authored Markdown.
              // Render it verbatim with preserved whitespace so single
              // newlines survive and characters like `*` stay literal.
              return message.role === 'user' ? (
                <div
                  key={index}
                  className="whitespace-pre-wrap text-slate-800"
                >
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
    </article>
  );
}
