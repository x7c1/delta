import { ApiError } from '@delta/api-client';
import { storeAuthToken, storedAuthToken } from './tokenStore';

/**
 * How the page stands with the server's per-run bearer token when it boots.
 *
 * - `ready` — a token is stored; the app renders and presents it.
 * - `missing` — the page was opened without a code and holds no token.
 * - `code-refused` — the code in the URL was used, expired or unknown.
 * - `token-refused` — the server answered `401` to the stored token (it was
 *   restarted and minted a new one). Reached at run time, not at boot.
 * - `exchange-failed` — the code could not be traded for another reason (the
 *   server is unreachable, or the browser will not store the token).
 */
export type AuthState =
  | { kind: 'ready' }
  | { kind: 'missing' }
  | { kind: 'code-refused' }
  | { kind: 'token-refused' }
  | { kind: 'exchange-failed'; message: string };

/** The fragment parameter the server's links carry the one-time code in. */
const CODE_PARAM = 'code';

/** The `code` parameter of the current URL's fragment, `null` when absent. */
function fragmentCode(): string | null {
  const hash = window.location.hash.replace(/^#/, '');
  if (!hash) {
    return null;
  }
  return new URLSearchParams(hash).get(CODE_PARAM);
}

/** Whether the current URL's fragment carries a one-time code. */
export function hasFragmentCode(): boolean {
  return fragmentCode() !== null;
}

/**
 * The one-time code in the current URL's fragment (`#code=<code>`), removed
 * from the address bar so it is not left in history, a bookmark, or a copied
 * link. `null` when the fragment carries none.
 */
function takeFragmentCode(): string | null {
  const code = fragmentCode();
  if (code === null) {
    return null;
  }
  const { pathname, search } = window.location;
  window.history.replaceState(window.history.state, '', `${pathname}${search}`);
  return code || null;
}

/** Trades a one-time code for the bearer token; `ApiClient.exchangeAuthCode`. */
export type ExchangeCode = (code: string) => Promise<string>;

/**
 * Settle the page's token at boot.
 *
 * A code in the URL is traded for the token, which is stored for this origin;
 * a refused code shows the screen asking for a new link, whatever is stored,
 * since a link that no longer works should say so. Without a code, a stored
 * token is used as is — a `401` to it later flips the app to `token-refused`.
 */
export async function establishAuth(
  exchange: ExchangeCode,
): Promise<AuthState> {
  const code = takeFragmentCode();
  if (code === null) {
    return storedAuthToken() ? { kind: 'ready' } : { kind: 'missing' };
  }
  let token: string;
  try {
    token = await exchange(code);
  } catch (error) {
    if (error instanceof ApiError && error.status === 401) {
      return { kind: 'code-refused' };
    }
    console.error('could not trade the one-time code for a token', error);
    return { kind: 'exchange-failed', message: describe(error) };
  }
  try {
    storeAuthToken(token);
  } catch (error) {
    console.error('could not store the token', error);
    return {
      kind: 'exchange-failed',
      message: 'The browser refused to store the sign-in for this page.',
    };
  }
  return { kind: 'ready' };
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
