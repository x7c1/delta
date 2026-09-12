import type { Components } from 'react-markdown';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { remarkTrimAutolinkPunctuation } from './remarkTrimAutolinkPunctuation';

/**
 * Everything react-markdown computed for the anchor is passed through — `href`
 * and `title`, but also the `id`, `aria-*` and `data-footnote-*` attributes it
 * puts on GFM footnote links, which naming the props one by one would silently
 * drop. The exception is its mdast `node`: that is react-markdown's own handle,
 * not an HTML attribute, and React warns about an unknown attribute once it
 * reaches the DOM. `target` and `rel` come last so nothing out of the Markdown
 * can displace them.
 *
 * Built once at module scope: a fresh object per render would give `a` a new
 * component identity each time, remounting every link as the streaming bubble
 * re-renders on each token.
 */
const components: Components = {
  a: (props) => {
    const anchorProps = { ...props };
    delete anchorProps.node;
    return <a {...anchorProps} target="_blank" rel="noopener noreferrer" />;
  },
};

/**
 * Renders assistant prose as Markdown. The single source of truth for how
 * assistant text is displayed, shared by the persisted transcript message and
 * the live streaming bubble so the two look identical across the handoff.
 *
 * A small, chat-tuned Markdown stylesheet scoped to the `markdown-body` class
 * (see index.css) styles just the elements Claude emits, rather than a full
 * typography framework. GFM enables tables, strikethrough, task lists, and
 * autolinks, which Claude routinely emits; `remarkTrimAutolinkPunctuation`
 * then moves the CJK text the autolinker absorbs back into the prose.
 *
 * Every link opens in a new tab, matching the session card's pull-request
 * link, so that following one never navigates the conversation away and costs
 * the user the screen they were reading. The rule has no exceptions by link
 * shape — relative and `#fragment` links get it too — because assistant prose
 * is not expected to link into Delta's own UI. GFM footnotes are the one place
 * a `#fragment` link is written by the renderer rather than by the agent:
 * those anchors follow the same rule, so a footnote marker opens a tab instead
 * of jumping within the message.
 */
export function AssistantMarkdown({ text }: { text: string }) {
  return (
    <div className="markdown-body text-fg">
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkTrimAutolinkPunctuation]}
        components={components}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}
