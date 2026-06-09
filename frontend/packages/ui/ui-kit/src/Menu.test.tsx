import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { Menu } from './Menu';

describe('Menu', () => {
  it('opens the panel when the trigger is clicked', () => {
    render(<Menu label="Session actions" items={[{ label: 'Close', onSelect: vi.fn() }]} />);

    const trigger = screen.getByRole('button', { name: 'Session actions' });
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();

    fireEvent.click(trigger);

    expect(trigger).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByRole('menu')).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Close' })).toBeInTheDocument();
  });

  it('runs an item onSelect and closes the panel', () => {
    const onSelect = vi.fn();
    render(<Menu label="Session actions" items={[{ label: 'Close', onSelect }]} />);

    fireEvent.click(screen.getByRole('button', { name: 'Session actions' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Close' }));

    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('closes on Escape', () => {
    render(<Menu label="Session actions" items={[{ label: 'Close', onSelect: vi.fn() }]} />);

    fireEvent.click(screen.getByRole('button', { name: 'Session actions' }));
    expect(screen.getByRole('menu')).toBeInTheDocument();

    fireEvent.keyDown(document, { key: 'Escape' });

    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('closes on click outside', () => {
    render(
      <div>
        <span data-testid="outside">outside</span>
        <Menu label="Session actions" items={[{ label: 'Close', onSelect: vi.fn() }]} />
      </div>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Session actions' }));
    expect(screen.getByRole('menu')).toBeInTheDocument();

    fireEvent.pointerDown(screen.getByTestId('outside'));

    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('notifies onOpenChange when the panel opens and closes', () => {
    const onOpenChange = vi.fn();
    render(
      <Menu
        label="Session actions"
        items={[{ label: 'Close', onSelect: vi.fn() }]}
        onOpenChange={onOpenChange}
      />,
    );

    // Mounted closed: the callback fires once with the initial state.
    expect(onOpenChange).toHaveBeenLastCalledWith(false);

    fireEvent.click(screen.getByRole('button', { name: 'Session actions' }));
    expect(onOpenChange).toHaveBeenLastCalledWith(true);

    fireEvent.keyDown(document, { key: 'Escape' });
    expect(onOpenChange).toHaveBeenLastCalledWith(false);
  });

  it('does not steal focus to the trigger on mount', () => {
    render(<Menu label="Session actions" items={[{ label: 'Close', onSelect: vi.fn() }]} />);

    // A freshly mounted Menu must not grab focus, or a page load would draw a
    // focus ring on a kebab the user never interacted with.
    const trigger = screen.getByRole('button', { name: 'Session actions' });
    expect(trigger).not.toHaveFocus();
    expect(document.body).toHaveFocus();
  });

  it('restores focus to the trigger after the panel closes', () => {
    render(<Menu label="Session actions" items={[{ label: 'Close', onSelect: vi.fn() }]} />);

    const trigger = screen.getByRole('button', { name: 'Session actions' });
    fireEvent.click(trigger);
    fireEvent.keyDown(document, { key: 'Escape' });

    // Closing after having been open returns focus to the trigger (keyboard UX).
    expect(trigger).toHaveFocus();
  });

  it('does not open when disabled', () => {
    render(
      <Menu
        label="Session actions"
        disabled
        items={[{ label: 'Close', onSelect: vi.fn() }]}
      />,
    );

    const trigger = screen.getByRole('button', { name: 'Session actions' });
    expect(trigger).toBeDisabled();

    fireEvent.click(trigger);

    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('renders the trigger disabled when there are no items', () => {
    render(<Menu label="Session actions" items={[]} />);

    expect(screen.getByRole('button', { name: 'Session actions' })).toBeDisabled();
  });
});
