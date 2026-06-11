// Identifier aliases shared across the frontend.
//
// The wire shapes (generated into @delta/wire-gen) carry these ids as plain
// `string` / `number`; the aliases exist so frontend signatures and stores can
// say *which* id they mean. They are structural aliases, not brands, so the
// generated types and these aliases interoperate freely.

/** String identifier for a session. */
export type SessionId = string;

/** Server-issued integer identifier for a thread. */
export type ThreadId = number;

/** String identifier for a transcript message. */
export type MessageUuid = string;
