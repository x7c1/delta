/**
 * Which native shell, if any, the page is running inside.
 *
 * The desktop shells mark `<html data-shell="…">` before the page's scripts
 * run: `tauri-macos` from delta-desktop's `macos_title_bar.rs`, `tauri-linux`
 * from its `quit_shortcut.rs`. The browser does not.
 */
export const MACOS_SHELL = 'tauri-macos';
export const LINUX_SHELL = 'tauri-linux';

/** Whether the page is inside the macOS desktop shell. */
export function isMacosShell(): boolean {
  return document.documentElement.dataset.shell === MACOS_SHELL;
}

/**
 * What the document title ends with while the terminal has focus. The Linux
 * shell follows the title (see `quit_shortcut.rs`, which holds the same
 * string) and lets Ctrl-Q through to the terminal instead of quitting.
 */
export const TERMINAL_FOCUSED_TITLE_SUFFIX = ' [terminal focused]';

/**
 * Tell the Linux shell whether the terminal has focus, by adding or removing
 * {@link TERMINAL_FOCUSED_TITLE_SUFFIX} on the document title. Does nothing
 * outside the Linux shell, so the browser's tab title never changes.
 */
export function reportTerminalFocus(focused: boolean): void {
  if (document.documentElement.dataset.shell !== LINUX_SHELL) {
    return;
  }
  const base = document.title.endsWith(TERMINAL_FOCUSED_TITLE_SUFFIX)
    ? document.title.slice(0, -TERMINAL_FOCUSED_TITLE_SUFFIX.length)
    : document.title;
  document.title = focused ? base + TERMINAL_FOCUSED_TITLE_SUFFIX : base;
}
