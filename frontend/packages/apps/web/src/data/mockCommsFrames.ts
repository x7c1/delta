import type { CommsFrame } from '@delta/wire-gen';

/**
 * Mock-mode source for the comms-log pane: MSW cannot mock a WebSocket, so a
 * scripted exchange stands in for the `/comms` stream (see `mockCommsFrames`).
 *
 * `@delta/api-mocks` is loaded with a dynamic import so it never reaches the
 * production bundle: the static import above is type-only (erased at compile
 * time), and the value import happens inside {@link loadMockCommsFrames}, which
 * only runs in mock mode.
 */
export async function loadMockCommsFrames(): Promise<CommsFrame[]> {
  const { mockCommsFrames } = await import('@delta/api-mocks');
  return mockCommsFrames();
}
