import { afterEach, describe, expect, it, vi } from 'vitest';
import { handleUndoKey, installUndoKeys, type ExecCommand } from './undoKeys';
import { LINUX_SHELL, MACOS_SHELL } from './shell';

/** Dispatch a keydown on `target` with `handleUndoKey` listening on it. */
function press(target: HTMLElement, init: KeyboardEventInit): { exec: ExecCommand; event: KeyboardEvent } {
  const exec = vi.fn<ExecCommand>();
  const event = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });
  const listener = (e: KeyboardEvent) => handleUndoKey(e, exec);
  document.addEventListener('keydown', listener);
  target.dispatchEvent(event);
  document.removeEventListener('keydown', listener);
  return { exec, event };
}

function mount<T extends HTMLElement>(element: T, parent: HTMLElement = document.body): T {
  parent.appendChild(element);
  return element;
}

afterEach(() => {
  document.body.innerHTML = '';
  delete document.documentElement.dataset.shell;
  vi.restoreAllMocks();
});

describe('handleUndoKey', () => {
  it('runs undo for Ctrl+Z in a textarea and prevents the default', () => {
    const { exec, event } = press(mount(document.createElement('textarea')), { key: 'z', ctrlKey: true });
    expect(exec).toHaveBeenCalledWith('undo');
    expect(event.defaultPrevented).toBe(true);
  });

  it('runs redo for Ctrl+Shift+Z', () => {
    const { exec, event } = press(mount(document.createElement('textarea')), {
      key: 'Z',
      ctrlKey: true,
      shiftKey: true,
    });
    expect(exec).toHaveBeenCalledWith('redo');
    expect(event.defaultPrevented).toBe(true);
  });

  it('runs undo in a text input, a number input, and a contenteditable element', () => {
    const input = mount(document.createElement('input'));
    expect(press(input, { key: 'z', ctrlKey: true }).exec).toHaveBeenCalledWith('undo');

    const number = mount(Object.assign(document.createElement('input'), { type: 'number' }));
    expect(press(number, { key: 'z', ctrlKey: true }).exec).toHaveBeenCalledWith('undo');

    const host = mount(document.createElement('div'));
    host.setAttribute('contenteditable', 'true');
    const child = mount(document.createElement('span'), host);
    expect(press(child, { key: 'z', ctrlKey: true }).exec).toHaveBeenCalledWith('undo');
  });

  it('leaves a key during IME composition alone', () => {
    const { exec, event } = press(mount(document.createElement('textarea')), {
      key: 'z',
      ctrlKey: true,
      isComposing: true,
    });
    expect(exec).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(false);
  });

  it('leaves a non-editable target alone', () => {
    const targets: HTMLElement[] = [
      mount(document.createElement('button')),
      mount(Object.assign(document.createElement('input'), { type: 'checkbox' })),
      mount(Object.assign(document.createElement('textarea'), { readOnly: true })),
    ];
    const notEditable = mount(document.createElement('div'));
    notEditable.setAttribute('contenteditable', 'false');
    targets.push(notEditable);
    for (const target of targets) {
      const { exec, event } = press(target, { key: 'z', ctrlKey: true });
      expect(exec).not.toHaveBeenCalled();
      expect(event.defaultPrevented).toBe(false);
    }
  });

  it("leaves the terminal's helper textarea alone", () => {
    const terminal = mount(document.createElement('div'));
    terminal.className = 'xterm';
    const helper = mount(document.createElement('textarea'), terminal);
    helper.className = 'xterm-helper-textarea';
    const { exec, event } = press(helper, { key: 'z', ctrlKey: true });
    expect(exec).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(false);
  });

  it('leaves Alt and Meta combinations alone', () => {
    const textarea = mount(document.createElement('textarea'));
    for (const init of [
      { key: 'z', ctrlKey: true, altKey: true },
      { key: 'z', ctrlKey: true, metaKey: true },
      { key: 'z', metaKey: true },
      { key: 'z' },
    ]) {
      const { exec, event } = press(textarea, init);
      expect(exec).not.toHaveBeenCalled();
      expect(event.defaultPrevented).toBe(false);
    }
  });
});

describe('installUndoKeys', () => {
  it('installs nothing outside the Linux shell', () => {
    const add = vi.spyOn(document, 'addEventListener');
    expect(installUndoKeys()).toBe(false);
    document.documentElement.dataset.shell = MACOS_SHELL;
    expect(installUndoKeys()).toBe(false);
    expect(add).not.toHaveBeenCalled();
  });

  it('runs the page undo for Ctrl+Z inside the Linux shell', () => {
    document.documentElement.dataset.shell = LINUX_SHELL;
    const execCommand = vi.fn(() => true);
    Object.defineProperty(document, 'execCommand', { configurable: true, value: execCommand });
    const add = vi.spyOn(document, 'addEventListener');
    try {
      expect(installUndoKeys()).toBe(true);
      const event = new KeyboardEvent('keydown', { key: 'z', ctrlKey: true, bubbles: true, cancelable: true });
      mount(document.createElement('textarea')).dispatchEvent(event);
      expect(execCommand).toHaveBeenCalledWith('undo');
      expect(event.defaultPrevented).toBe(true);
    } finally {
      for (const [type, listener] of add.mock.calls) {
        document.removeEventListener(type, listener);
      }
      delete (document as { execCommand?: unknown }).execCommand;
    }
  });
});
