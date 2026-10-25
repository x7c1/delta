/**
 * Request helpers for the handler tests: run one mock handler, selected by
 * method and path suffix, against a request, returning the raw response.
 */
import type { HttpHandler } from 'msw';

/**
 * Run a handler selected by method and path suffix, returning the raw response.
 * Backs {@link runPost}; {@link runGet} stays separate because a GET carries no
 * body and no content-type.
 */
export async function runWithMethod(
  handlers: HttpHandler[],
  method: 'POST' | 'PATCH' | 'DELETE',
  pathSuffix: string,
  url: string,
  body?: unknown,
): Promise<Response> {
  const handler = handlers.find(
    (h) => h.info.method === method && String(h.info.path).endsWith(pathSuffix),
  );
  if (!handler) {
    throw new Error(`${method} handler ending in ${pathSuffix} not found`);
  }
  const request = new Request(url, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const result = await handler.run({ request, requestId: 'test' });
  const response = result?.response;
  if (!response) {
    throw new Error('handler did not produce a response');
  }
  return response;
}

/** Run a POST handler selected by a path suffix, returning the raw response. */
export async function runPost(
  handlers: HttpHandler[],
  pathSuffix: string,
  url: string,
  body?: unknown,
): Promise<Response> {
  return runWithMethod(handlers, 'POST', pathSuffix, url, body);
}

/** Run a GET handler selected by a path pattern suffix. */
export async function runGet(
  handlers: HttpHandler[],
  pathSuffix: string,
  url: string,
): Promise<Response> {
  const handler = handlers.find(
    (h) => h.info.method === 'GET' && String(h.info.path).endsWith(pathSuffix),
  );
  if (!handler) {
    throw new Error(`GET handler ending in ${pathSuffix} not found`);
  }
  const result = await handler.run({ request: new Request(url), requestId: 'test' });
  const response = result?.response;
  if (!response) {
    throw new Error('handler did not produce a response');
  }
  return response;
}
