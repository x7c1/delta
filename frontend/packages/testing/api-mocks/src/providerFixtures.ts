import type { ProviderAvailability } from '@delta/wire-gen';

/**
 * Per-provider launch availability and capability profile for
 * `GET /api/providers`. Both providers are available by default so the
 * new-session provider selector is fully usable with no backend and existing
 * tests / e2e are unaffected. Capabilities mirror the real backend: Claude
 * offers an attachable terminal (`has_terminal: true`) and takes its launch
 * options as CLI flags, Codex is headless (`has_terminal: false`, and therefore
 * `has_comms_log: true` — its right-pane window is the frame log instead) and
 * takes them as session-start request fields. The workspace picks the right pane
 * from those two flags, and Settings words its launch-option form from the style.
 * Codex also accepts a session-scoped permission decision
 * (`has_allow_for_session: true`), which is what puts the "Allow for session"
 * button on its approval notices and keeps it off Claude's.
 * A test that needs an unavailable provider overrides this handler (see
 * `createHandlers`).
 */
export function mockProviders(): ProviderAvailability[] {
  return [
    {
      provider: 'claude',
      available: true,
      detail: null,
      capabilities: {
        has_terminal: true,
        has_comms_log: false,
        has_allow_for_session: false,
        launch_option_style: 'cli_flag',
      },
    },
    {
      provider: 'codex',
      available: true,
      detail: null,
      capabilities: {
        has_terminal: false,
        has_comms_log: true,
        has_allow_for_session: true,
        launch_option_style: 'request_field',
      },
    },
  ];
}
