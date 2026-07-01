import { useCallback } from 'react';
import { ApiError, useOpenCwdMutation } from '@delta/api-client';
import { useApiClient } from '../../data/apiContext';
import { useNotificationStore } from '../../store/notificationStore';

/**
 * The single handler currently exposed to the UI. Kept as a constant so a
 * future menu (or a settings-picker default) has one canonical string to
 * refer to, and the entry-point components do not each hard-code it.
 */
export const DEFAULT_OPEN_CWD_HANDLER = 'vscode';

/**
 * Human-facing label for the default handler, used in menu items,
 * `aria-label`s and error messages. Kept alongside the id so the two never
 * drift.
 */
export const DEFAULT_OPEN_CWD_HANDLER_LABEL = 'VS Code';

/**
 * A callback the entry-point components (message meta cwd, session menu)
 * call to launch the current external tool at `path`.
 *
 * Success is silent — the editor opening is the feedback. Errors are
 * translated to the app-wide {@link useNotificationStore} snackbar with a
 * specific line for the common causes (VS Code missing, path rejected)
 * and a generic line for anything else.
 *
 * The two call sites converge on this hook rather than each embedding the
 * mutation + error mapping so a future addition (a second handler, a
 * different feedback surface) has exactly one place to update.
 */
export function useOpenCwd() {
  const client = useApiClient();
  const openCwd = useOpenCwdMutation(client);
  const showError = useNotificationStore((state) => state.showError);

  return useCallback(
    (path: string) => {
      openCwd.mutate(
        { path, handler: DEFAULT_OPEN_CWD_HANDLER },
        {
          onError: (err: unknown) => {
            const title = `Could not open in ${DEFAULT_OPEN_CWD_HANDLER_LABEL}`;
            if (err instanceof ApiError) {
              switch (err.code) {
                case 'open_cwd_command_not_found':
                  showError(
                    title,
                    `${DEFAULT_OPEN_CWD_HANDLER_LABEL} is not installed. Install its shell command ("code") and try again.`,
                  );
                  return;
                case 'open_cwd_path_not_allowed':
                  showError(title, 'The path is not known to this server.');
                  return;
                case 'open_cwd_unknown_handler':
                  showError(title, 'The requested handler is not registered.');
                  return;
                case 'open_cwd_spawn_failed':
                  showError(title, err.message);
                  return;
              }
              // Any other ApiError code: forward the server message verbatim
              // so a novel failure still surfaces useful text.
              showError(title, err.message);
              return;
            }
            // Non-API error (network, aborted fetch): use the exception
            // message when it is a real Error, and a generic fallback
            // otherwise so the user always sees something actionable.
            const detail =
              err instanceof Error ? err.message : 'The request failed.';
            showError(title, detail);
          },
        },
      );
    },
    [openCwd, showError],
  );
}
