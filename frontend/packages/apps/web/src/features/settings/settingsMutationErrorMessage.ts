import { ApiError } from '@delta/api-client';

/**
 * The message shown for a failed settings mutation. The server's own
 * explanation is appended when it sent one — {@link ApiError} carries the
 * parsed `error` field — so a user is told *why* rather than only that
 * something failed. A transport failure or an opaque status leaves just the
 * fallback sentence.
 *
 * Shared by the prompt-template editor and the launch-option add form: both
 * have refusals the caller can act on (a blank field, an option that may not be
 * enabled by default) whose reason only the server knows.
 */
export function settingsMutationErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof ApiError && error.message.length > 0) {
    return `${fallback} ${error.message}`;
  }
  return fallback;
}
