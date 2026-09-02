import type { ContentBlock } from '@delta/wire-gen';

/** A short one-line caption for a content block card. */
export function blockSummary(block: ContentBlock): string {
  switch (block.type) {
    case 'thinking':
      return 'thinking';
    case 'tool_use':
      return `tool: ${block.name}`;
    case 'tool_result':
      return block.is_error ? 'tool result (error)' : 'tool result';
    case 'text':
      return 'text';
    case 'other':
      return 'other';
  }
}

/** Render arbitrary tool input/output JSON to a readable string. */
export function stringifyContent(value: unknown): string {
  if (typeof value === 'string') {
    return value;
  }
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}
