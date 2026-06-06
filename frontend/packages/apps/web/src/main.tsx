import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { isMockMode } from './config';
import './index.css';

async function bootstrap(): Promise<void> {
  if (isMockMode()) {
    const { startMockServiceWorker } = await import('./mocks/browser');
    await startMockServiceWorker();
  }

  const rootElement = document.getElementById('root');
  if (!rootElement) {
    throw new Error('Root element #root not found');
  }

  createRoot(rootElement).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

void bootstrap();
