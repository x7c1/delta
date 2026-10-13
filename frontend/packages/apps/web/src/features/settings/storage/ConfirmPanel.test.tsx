import { useRef, useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { Button, Dialog } from '@delta/ui-kit';
import { ConfirmPanel } from './ConfirmPanel';

/**
 * A Settings-like dialog holding one Storage control that opens a
 * confirmation in its place, as the Storage blocks do.
 */
function SettingsWithPanel({ onClose, pending = false }: { onClose: () => void; pending?: boolean }) {
  const [confirming, setConfirming] = useState(false);
  const openerRef = useRef<HTMLDivElement>(null);
  return (
    <Dialog open onClose={onClose} title="Settings">
      {confirming ? (
        <ConfirmPanel
          confirmLabel="Remove"
          onConfirm={() => {}}
          onCancel={() => setConfirming(false)}
          pending={pending}
          testId="panel"
          returnFocusTo={openerRef}
        >
          <p>Remove it?</p>
        </ConfirmPanel>
      ) : (
        <div ref={openerRef}>
          <Button onClick={() => setConfirming(true)}>Remove…</Button>
        </div>
      )}
      <Button>Elsewhere</Button>
    </Dialog>
  );
}

describe('ConfirmPanel', () => {
  it('cancels on Escape and leaves the dialog open', () => {
    const onClose = vi.fn();
    render(<SettingsWithPanel onClose={onClose} />);
    fireEvent.click(screen.getByRole('button', { name: 'Remove…' }));
    const cancel = screen.getByRole('button', { name: 'Cancel' });
    expect(cancel).toHaveFocus();

    fireEvent.keyDown(cancel, { key: 'Escape' });

    expect(screen.queryByTestId('panel')).toBeNull();
    expect(screen.getByRole('button', { name: 'Remove…' })).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it('cancels on Escape even with the focus outside it', () => {
    const onClose = vi.fn();
    render(<SettingsWithPanel onClose={onClose} />);
    fireEvent.click(screen.getByRole('button', { name: 'Remove…' }));
    const elsewhere = screen.getByRole('button', { name: 'Elsewhere' });
    elsewhere.focus();

    fireEvent.keyDown(elsewhere, { key: 'Escape' });

    expect(screen.queryByTestId('panel')).toBeNull();
    expect(onClose).not.toHaveBeenCalled();
  });

  it('gives the focus back to the control that opened it when cancelled', () => {
    render(<SettingsWithPanel onClose={() => {}} />);
    fireEvent.click(screen.getByRole('button', { name: 'Remove…' }));
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(screen.getByRole('button', { name: 'Remove…' })).toHaveFocus();

    fireEvent.click(screen.getByRole('button', { name: 'Remove…' }));
    fireEvent.keyDown(screen.getByRole('button', { name: 'Cancel' }), { key: 'Escape' });

    expect(screen.getByRole('button', { name: 'Remove…' })).toHaveFocus();
  });

  it('leaves the focus where it is when Escape cancels from outside it', () => {
    render(<SettingsWithPanel onClose={() => {}} />);
    fireEvent.click(screen.getByRole('button', { name: 'Remove…' }));
    const elsewhere = screen.getByRole('button', { name: 'Elsewhere' });
    elsewhere.focus();

    fireEvent.keyDown(elsewhere, { key: 'Escape' });

    expect(elsewhere).toHaveFocus();
  });

  it('lets Escape close the dialog once no panel is open', () => {
    const onClose = vi.fn();
    render(<SettingsWithPanel onClose={onClose} />);
    fireEvent.click(screen.getByRole('button', { name: 'Remove…' }));
    fireEvent.keyDown(screen.getByRole('button', { name: 'Cancel' }), { key: 'Escape' });
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.keyDown(screen.getByRole('button', { name: 'Remove…' }), { key: 'Escape' });

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('neither cancels nor closes the dialog while the request is in flight', () => {
    const onClose = vi.fn();
    render(<SettingsWithPanel onClose={onClose} pending />);
    fireEvent.click(screen.getByRole('button', { name: 'Remove…' }));

    fireEvent.keyDown(screen.getByRole('alertdialog'), { key: 'Escape' });

    expect(screen.getByTestId('panel')).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });
});
