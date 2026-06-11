// A discriminated narrowing of the generated `CreateSendRequest` wire type.
//
// The wire shape (generated/CreateSendRequest.ts) is a single struct whose
// `thread_id` / `new_session` fields are mutually exclusive — a constraint the
// server enforces at runtime (400 on conflict) but a flat struct cannot
// express. These narrowings encode the two valid forms so callers cannot
// construct a conflicting body. `SendRequestIsWireCompatible` (below) makes the
// compiler reject any narrowing that drifts from the generated wire shape.

import type { CreateSendRequest } from './generated/CreateSendRequest';

/**
 * Send target addressing an existing session via one of its threads. A branch
 * send additionally sets `semantic_parent_uuid` (and still requires `thread_id`).
 */
export interface SendToThread {
  thread_id: number;
  text: string;
  locator_quote?: string;
  semantic_parent_uuid?: string;
}

/**
 * Send target that spawns a brand-new session. The first message lands on the
 * new session's main thread; there is no `thread_id` yet. `locator_quote` is
 * ignored by the server for this target, so it is intentionally not modelled.
 */
export interface SendToNewSession {
  new_session: true;
  text: string;
  /**
   * The working directory the fresh session should start in. Honored only for a
   * new-session send; when omitted the server uses its default per-spawn
   * directory.
   */
  workdir?: string;
}

/** Request body for `POST /api/sends` — a discriminated send target. */
export type SendRequest = SendToThread | SendToNewSession;

/**
 * Compile-time guard: every {@link SendRequest} form must stay assignable to
 * the generated wire shape. If the Rust contract changes (`make gen`), this
 * conditional type becomes `never` and the exported assertion below fails to
 * typecheck.
 */
export type SendRequestIsWireCompatible =
  SendRequest extends CreateSendRequest ? true : never;

// Referencing the guard in an exported value position forces the compiler to
// evaluate it even under `isolatedModules`.
export const SEND_REQUEST_IS_WIRE_COMPATIBLE: SendRequestIsWireCompatible =
  true;
