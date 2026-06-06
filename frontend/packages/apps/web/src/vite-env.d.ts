/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** When `'1'`, start MSW + the WS fake so the app runs with no backend. */
  readonly VITE_API_MOCK?: string;
  /** Base URL for the REST surface. Defaults to same-origin. */
  readonly VITE_API_BASE_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
