/**
 * Which native shell, if any, the page is running inside.
 *
 * The macOS desktop shell marks `<html data-shell="tauri-macos">` before the
 * page's scripts run (see delta-desktop's `macos_title_bar.rs`); the browser and
 * the Linux shell do not.
 */
export const MACOS_SHELL = 'tauri-macos';

/** Whether the page is inside the macOS desktop shell. */
export function isMacosShell(): boolean {
  return document.documentElement.dataset.shell === MACOS_SHELL;
}
