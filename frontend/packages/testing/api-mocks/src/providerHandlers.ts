import { http, HttpResponse, type RequestHandler } from 'msw';
import type { ProvidersResponse } from '@delta/wire-gen';
import { mockProviders } from './providerFixtures';

/** The provider availability route. */
export function providerHandlers(): RequestHandler[] {
  return [
    // Per-provider launch availability for the new-session selector. Both
    // providers are available by default so the selector is fully usable with no
    // backend; a test overrides this handler to make a provider unavailable.
    http.get('*/api/providers', () => {
      const responseBody: ProvidersResponse = { providers: mockProviders() };
      return HttpResponse.json(responseBody);
    }),
  ];
}
