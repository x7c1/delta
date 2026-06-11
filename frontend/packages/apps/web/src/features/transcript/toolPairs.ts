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

/** Whether a single content block renders to nothing on its own. */
function blockRendersNothing(
  block: ContentBlock,
  pairing: ToolPairing | undefined,
): boolean {
  switch (block.type) {
    case 'thinking':
      // Claude Code records a signed reference for thinking but leaves the
      // plaintext empty, so an empty thinking block renders nothing.
      return block.thinking.trim() === '';
    case 'tool_result':
      // A result paired to a visible call is shown inline with that call; with
      // no pairing it is treated as an orphan and rendered, so it is not nothing.
      return pairing?.toolUseIds.has(block.tool_use_id) ?? false;
    default:
      // text, tool_use, and any other block always render something.
      return false;
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
  return message.content.every((block) => blockRendersNothing(block, pairing));
}
