import { useCallback } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import type { Message } from '@delta/model';
import { Badge, Collapsible } from '@delta/ui-kit';
import { formatLocalDateTime } from '../../utils/formatLocalDateTime';
import { blockSummary, stringifyContent } from './blockSummary';
import { messageRendersNothing, type ToolPairing } from './toolPairs';

export interface MessageItemProps {
  message: Message;
  /** Called when the user selects a text range within this message. */
  onSelectQuote?: (message: Message, quote: string) => void;
  /**
   * The thread's tool_use ⇄ tool_result linkage. When provided, a `tool_use`
   * block renders together with its result, and a `tool_result` whose call is
   * shown elsewhere is suppressed here (it is rendered inline with that call).
   */
  pairing?: ToolPairing;
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
 *
 * Claude's transcript delivers tool results as `role: "user"` lines, so a
 * user-role message that carries no human-authored text is a tool-result
 * carrier, not a real user turn. Those render on the assistant side (left,
 * plain) so they don't masquerade as right-aligned user bubbles.
 */
export function MessageItem({
  message,
  onSelectQuote,
  pairing,
}: MessageItemProps) {
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

  const timestamp = formatLocalDateTime(message.created_at, true);
  // A genuine human turn is a user-role message with author-written text. A
  // user-role message that only carries tool results (fed back to the model
  // after a tool_use) is not a human turn — render it on the assistant side.
  const isUserTurn =
    message.role === 'user' &&
    message.content.some((block) => block.type === 'text');

  const renderedBlocks = message.content.map((block, index) => {
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
          <div
            key={index}
            // A small, chat-tuned Markdown stylesheet scoped to this class
            // (see index.css) — just the elements Claude emits, styled
            // modestly, rather than a full typography framework.
            className="markdown-body text-slate-800"
          >
            {/* GFM enables tables, strikethrough, task lists, and
                autolinks, which Claude routinely emits. */}
            <ReactMarkdown remarkPlugins={[remarkGfm]}>
              {block.text}
            </ReactMarkdown>
          </div>
        );
      case 'thinking':
        // Claude Code records a signed reference for thinking but leaves
        // the plaintext empty in the transcript, so a thinking block
        // usually has no body. A collapsible that always expands to
        // nothing is noise — render only when there is text to show.
        if (!block.thinking.trim()) {
          return null;
        }
        return (
          <Collapsible key={index} summary={blockSummary(block)}>
            <pre className="whitespace-pre-wrap text-slate-600">
              {block.thinking}
            </pre>
          </Collapsible>
        );
      case 'tool_use': {
        // A tool call renders together with its result (resolved by id via
        // the pairing): the header names the tool and shows its outcome,
        // and the body stacks the call input above the returned result.
        const result = pairing?.resultByUseId.get(block.id);
        return (
          <Collapsible
            key={index}
            summary={
              <span className="flex items-center gap-1.5">
                <span className="font-medium text-slate-500">
                  {block.name}
                </span>
                {result ? (
                  result.is_error ? (
                    <Badge tone="warning">error</Badge>
                  ) : (
                    <span className="text-emerald-600" aria-label="ok">
                      ✓
                    </span>
                  )
                ) : (
                  <span className="text-slate-400">running…</span>
                )}
              </span>
            }
          >
            <div className="space-y-2">
              <div>
                <div className="text-[0.65rem] uppercase tracking-wide text-slate-400">
                  input
                </div>
                <pre className="whitespace-pre-wrap text-slate-700">
                  {stringifyContent(block.input)}
                </pre>
              </div>
              {result && (
                <div>
                  <div className="text-[0.65rem] uppercase tracking-wide text-slate-400">
                    result
                  </div>
                  <pre className="whitespace-pre-wrap text-slate-700">
                    {stringifyContent(result.content)}
                  </pre>
                </div>
              )}
            </div>
          </Collapsible>
        );
      }
      case 'tool_result':
        // A result paired to a call shown elsewhere is rendered inline with
        // that call (see 'tool_use' above), so suppress it here. Only an
        // orphan result (its call is not in view) falls through to a
        // standalone collapsible.
        if (pairing?.toolUseIds.has(block.tool_use_id)) {
          return null;
        }
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
  });

  // A message whose blocks all collapse to nothing — only an empty thinking
  // block, or only tool results that render inline with their calls — would
  // otherwise show as a bare timestamp. Render nothing for it. This mirrors the
  // transcript's own filter (the single source of truth is messageRendersNothing).
  if (messageRendersNothing(message, pairing)) {
    return null;
  }

  const blocks = (
    <div className="space-y-1.5" onMouseUp={handleMouseUp}>
      {renderedBlocks}
    </div>
  );

  // User: a right-aligned tinted bubble that does not span the full width.
  if (isUserTurn) {
    return (
      <article
        className="flex flex-col items-end px-3 text-sm"
        data-role={message.role}
        data-testid="message-item"
      >
        <div className="max-w-[85%] rounded-lg bg-blue-50 px-3 py-2 text-slate-800">
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

  // Assistant prose gets a tinted rounded bubble, in a different hue from the
  // user's bubble so the two sides are easy to tell apart. Tool turns and
  // tool-result carriers keep their own Collapsible cards instead — wrapping a
  // bubble around those would nest a box inside a box.
  const inBubble = !message.content.some(
    (block) => block.type === 'tool_use' || block.type === 'tool_result',
  );
  return (
    <article
      className="px-3 text-sm"
      data-role={message.role}
      data-testid="message-item"
    >
      {inBubble ? (
        <div className="rounded-lg bg-slate-50 px-3 py-2 text-slate-800">
          {blocks}
        </div>
      ) : (
        blocks
      )}
      {timestamp && (
        <span className="mt-1 block text-right text-xs tabular-nums text-slate-400">
          {timestamp}
        </span>
      )}
    </article>
  );
}
