import type { ContentBlock, Message, ToolResultBlock } from '@delta/wire-gen';

/**
 * The link between a tool invocation and its result.
 *
 * In Claude's transcript a `tool_use` block lives in an assistant message while
 * its `tool_result` arrives in the following `role: "user"` message, keyed by
 * `tool_use_id`. Pairing them is therefore a cross-message operation: it needs
 * every message in the thread, not a single one. This resolves that linkage so
 * a tool call can render together with its result.
 */
export interface ToolPairing {
  /** `tool_use.id` → its result block (gathered across all messages). */
  resultByUseId: Map<string, ToolResultBlock>;
  /** Every `tool_use.id` present, so a result can tell whether it is paired. */
  toolUseIds: Set<string>;
}

/** Build the tool_use ⇄ tool_result linkage across a thread's messages. */
export function buildToolPairing(messages: Message[]): ToolPairing {
  const resultByUseId = new Map<string, ToolResultBlock>();
  const toolUseIds = new Set<string>();
  for (const message of messages) {
    for (const block of message.content) {
      if (block.type === 'tool_use') {
        toolUseIds.add(block.id);
      } else if (block.type === 'tool_result') {
        resultByUseId.set(block.tool_use_id, block);
      }
    }
  }
  return { resultByUseId, toolUseIds };
}

/**
 * The shape a content block takes on screen. This is the one place that knows
 * how each block kind renders, so layout decisions can ask about the *shape*
 * rather than re-listing block types — a list that silently goes stale every
 * time a new kind is added.
 *
 * - `prose` — flows inline as the message's own words (assistant Markdown, or
 *   verbatim user text).
 * - `annotation` — a card that belongs to the surrounding prose turn rather than
 *   standing on its own: the model's thinking, which is part of the same
 *   utterance as the reply it accompanies.
 * - `card` — a standalone bordered card that *is* the message's substance rather
 *   than something the message says: tool activity, or a block kind this build
 *   cannot render.
 * - `nothing` — produces no output at all.
 */
export type BlockRendering = 'prose' | 'annotation' | 'card' | 'nothing';

/** How a single content block renders. */
function blockRendering(
  block: ContentBlock,
  pairing: ToolPairing | undefined,
): BlockRendering {
  switch (block.type) {
    case 'text':
      return 'prose';
    case 'thinking':
      // Claude Code records a signed reference for thinking but leaves the
      // plaintext empty, so an empty thinking block renders nothing.
      return block.thinking.trim() === '' ? 'nothing' : 'annotation';
    case 'tool_result':
      // A result paired to a visible call is shown inline with that call; with
      // no pairing it is treated as an orphan and rendered as its own card.
      return pairing?.toolUseIds.has(block.tool_use_id) ? 'nothing' : 'card';
    case 'tool_use':
    case 'other':
      return 'card';
  }
}

/**
 * True when a message has nothing that renders on its own: only empty thinking
 * blocks (Claude Code stores a signed reference but no plaintext) and/or tool
 * results already shown inline with their calls, or no content at all. The
 * transcript skips such a message so it does not emit an empty, padded turn,
 * which would otherwise show as a mysterious gap. `MessageItem` returns `null`
 * under the same condition — this is the single source of truth for both.
 */
export function messageRendersNothing(
  message: Message,
  pairing: ToolPairing | undefined,
): boolean {
  return message.content.every(
    (block) => blockRendering(block, pairing) === 'nothing',
  );
}

/**
 * True when what a message renders is prose — words it says — rather than
 * standalone cards.
 *
 * This is what decides whether the message is laid out inside the speech
 * bubble. The bubble is the container for prose, so it is earned by rendering
 * prose and lost by rendering anything that stands on its own: a message that
 * is only cards is machine activity, not speech, and wrapping a bubble around a
 * card would nest a box inside a box. An `annotation` (the model's thinking) is
 * part of the surrounding utterance, so it sits *inside* the bubble a reply
 * earns without earning one on its own — which is why a Claude reply carrying
 * both text and thinking keeps its bubble while a standalone reasoning message
 * (Codex delivers reasoning as its own message) does not.
 */
export function messageRendersProse(
  message: Message,
  pairing: ToolPairing | undefined,
): boolean {
  const renderings = message.content.map((block) =>
    blockRendering(block, pairing),
  );
  return renderings.includes('prose') && !renderings.includes('card');
}
