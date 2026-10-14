import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { ApiClient } from '@delta/api-client';
import { App } from './App';
import { apiBaseUrl, isMockMode } from './config';
import { establishAuth, type AuthState } from './features/auth/establishAuth';
import './index.css';

/**
 * How the page stands with the server's bearer token: in mock mode there is
 * no server to sign in to; otherwise a one-time code in the URL is traded for
 * the token, or the stored one is used.
 */
async function settleAuth(): Promise<AuthState> {
  if (isMockMode()) {
    return { kind: 'ready' };
  }
  const client = new ApiClient({ baseUrl: apiBaseUrl() });
  return establishAuth((code) => client.exchangeAuthCode(code));
}

async function bootstrap(): Promise<void> {
  if (isMockMode()) {
    const { startMockServiceWorker } = await import('./mocks/browser');
    await startMockServiceWorker();
  }
  const auth = await settleAuth();

  const rootElement = document.getElementById('root');
  if (!rootElement) {
    throw new Error('Root element #root not found');
  }

  createRoot(rootElement).render(
    <StrictMode>
      <App auth={auth} />
    </StrictMode>,
  );
}

void bootstrap();
