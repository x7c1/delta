import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

/**
 * Renders assistant prose as Markdown. The single source of truth for how
 * assistant text is displayed, shared by the persisted transcript message and
 * the live streaming bubble so the two look identical across the handoff.
 *
 * A small, chat-tuned Markdown stylesheet scoped to the `markdown-body` class
 * (see index.css) styles just the elements Claude emits, rather than a full
 * typography framework. GFM enables tables, strikethrough, task lists, and
 * autolinks, which Claude routinely emits.
 */
export function AssistantMarkdown({ text }: { text: string }) {
  return (
    <div className="markdown-body text-slate-800">
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{text}</ReactMarkdown>
    </div>
  );
}
