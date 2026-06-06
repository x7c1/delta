import { setupWorker } from 'msw/browser';
import { handlers } from '@delta/api-mocks';

/** MSW worker for the browser (dev mock mode). */
export const worker = setupWorker(...handlers);

/** Start MSW so the app runs with no backend. Idempotent per page load. */
export async function startMockServiceWorker(): Promise<void> {
  await worker.start({
    onUnhandledRequest: 'bypass',
    quiet: true,
  });
}
