// Named aliases for the members of the generated `ContentBlock` union.
//
// The wire shape itself is generated from the backend (`generated/
// ContentBlock.ts`); these aliases only name its members via `Extract`, so
// they can never drift from the Rust contract — if a variant is renamed or
// removed, the corresponding alias degenerates to `never` and consuming code
// stops compiling.

import type { ContentBlock } from './generated/ContentBlock';

/** Plain assistant or user text. */
export type TextBlock = Extract<ContentBlock, { type: 'text' }>;

/** Extended-thinking text emitted by the model. */
export type ThinkingBlock = Extract<ContentBlock, { type: 'thinking' }>;

/** A request from the model to invoke a tool. */
export type ToolUseBlock = Extract<ContentBlock, { type: 'tool_use' }>;

/** The result of a previously requested tool invocation. */
export type ToolResultBlock = Extract<ContentBlock, { type: 'tool_result' }>;

/** Any block kind the server does not model is preserved as `{ type: 'other' }`. */
export type OtherBlock = Extract<ContentBlock, { type: 'other' }>;
