import type { Message, ToolResultBlock } from '@delta/model';

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
 * True when a message carries nothing but tool results that are already paired
 * to a tool_use rendered elsewhere. Such a message renders to nothing on its
 * own (its results are shown inline with their calls), so the transcript skips
 * it rather than emit an empty turn.
 */
export function isAbsorbedToolResultMessage(
  message: Message,
  pairing: ToolPairing,
): boolean {
  return (
    message.content.length > 0 &&
    message.content.every(
      (block) =>
        block.type === 'tool_result' &&
        pairing.toolUseIds.has(block.tool_use_id),
    )
  );
}
