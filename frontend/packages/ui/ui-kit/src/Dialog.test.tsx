import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { Dialog } from './Dialog';

describe('Dialog', () => {
  it('renders nothing when closed and the panel when open', () => {
    const { rerender } = render(
      <Dialog open={false} onClose={vi.fn()} title="Choose a directory">
        <p>body</p>
      </Dialog>,
    );
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();

    rerender(
      <Dialog open onClose={vi.fn()} title="Choose a directory">
        <p>body</p>
      </Dialog>,
    );
    const dialog = screen.getByRole('dialog');
    expect(dialog).toHaveAttribute('aria-modal', 'true');
    expect(dialog).toHaveAccessibleName('Choose a directory');
    expect(screen.getByText('body')).toBeInTheDocument();
  });

  it('calls onClose on Escape', () => {
    const onClose = vi.fn();
    render(
      <Dialog open onClose={onClose} title="Choose a directory">
        <p>body</p>
      </Dialog>,
    );

    fireEvent.keyDown(document, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('calls onClose on a backdrop click', () => {
    const onClose = vi.fn();
    render(
      <Dialog open onClose={onClose} title="Choose a directory">
        <p>body</p>
      </Dialog>,
    );

    fireEvent.click(screen.getByTestId('dialog-backdrop'));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('does not call onClose on Escape or a backdrop click when not dismissable', () => {
    const onClose = vi.fn();
    render(
      <Dialog open dismissable={false} onClose={onClose} title="Choose a directory">
        <p>body</p>
      </Dialog>,
    );

    fireEvent.keyDown(document, { key: 'Escape' });
    fireEvent.click(screen.getByTestId('dialog-backdrop'));
    expect(onClose).not.toHaveBeenCalled();
  });

  it('does not call onClose when the content is clicked', () => {
    const onClose = vi.fn();
    render(
      <Dialog open onClose={onClose} title="Choose a directory">
        <p>body</p>
      </Dialog>,
    );

    fireEvent.click(screen.getByText('body'));
    expect(onClose).not.toHaveBeenCalled();
  });

  it('moves focus into the dialog on open and restores it on close', () => {
    const onClose = vi.fn();
    const trigger = document.createElement('button');
    document.body.appendChild(trigger);
    trigger.focus();
    expect(trigger).toHaveFocus();

    const { rerender } = render(
      <Dialog open onClose={onClose} title="Choose a directory">
        <p>body</p>
      </Dialog>,
    );
    expect(screen.getByRole('dialog')).toHaveFocus();

    rerender(
      <Dialog open={false} onClose={onClose} title="Choose a directory">
        <p>body</p>
      </Dialog>,
    );
    expect(trigger).toHaveFocus();
    trigger.remove();
  });

  it('renders the footer slot', () => {
    render(
      <Dialog
        open
        onClose={vi.fn()}
        title="Choose a directory"
        footer={<button type="button">Select</button>}
      >
        <p>body</p>
      </Dialog>,
    );

    expect(screen.getByRole('button', { name: 'Select' })).toBeInTheDocument();
  });
});
