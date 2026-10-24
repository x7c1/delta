import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
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

  it('keeps each item label on a single line', () => {
    // `Copy session ID` is wider than the panel's `min-w-[8rem]`, so without
    // `whitespace-nowrap` it wraps to a second line. The class is the contract.
    render(
      <Menu
        label="Session actions"
        items={[{ label: 'Copy session ID', onSelect: vi.fn() }]}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Session actions' }));

    const item = screen.getByRole('menuitem', { name: 'Copy session ID' });
    expect(item.className).toContain('whitespace-nowrap');
  });

  describe('with openOnHover', () => {
    afterEach(() => {
      vi.useRealTimers();
    });

    function renderHoverMenu(onSelect = vi.fn()) {
      render(
        <div>
          <input aria-label="Composer" />
          <Menu
            label="Session actions"
            openOnHover
            items={[{ label: 'Close', onSelect }]}
          />
        </div>,
      );
      const trigger = screen.getByRole('button', { name: 'Session actions' });
      // The pointer handlers sit on the wrapper around the trigger and panel.
      return { trigger, wrapper: trigger.parentElement as HTMLElement };
    }

    it('opens when a mouse enters the trigger, without moving focus', () => {
      const { trigger, wrapper } = renderHoverMenu();
      const composer = screen.getByRole('textbox', { name: 'Composer' });
      composer.focus();

      fireEvent.pointerEnter(wrapper, { pointerType: 'mouse' });

      expect(trigger).toHaveAttribute('aria-expanded', 'true');
      expect(screen.getByRole('menu')).toBeInTheDocument();
      // Hovering past a row must not pull focus out of the composer.
      expect(composer).toHaveFocus();
    });

    it('closes shortly after the pointer leaves, and stays open if it comes back', () => {
      vi.useFakeTimers();
      const { wrapper } = renderHoverMenu();

      fireEvent.pointerEnter(wrapper, { pointerType: 'mouse' });
      fireEvent.pointerLeave(wrapper, { pointerType: 'mouse' });
      // Crossing the gap between trigger and panel: back inside before the
      // delay runs out, so the panel stays.
      fireEvent.pointerEnter(wrapper, { pointerType: 'mouse' });
      act(() => vi.advanceTimersByTime(1000));
      expect(screen.getByRole('menu')).toBeInTheDocument();

      fireEvent.pointerLeave(wrapper, { pointerType: 'mouse' });
      expect(screen.getByRole('menu')).toBeInTheDocument();
      act(() => vi.advanceTimersByTime(1000));
      expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    });

    it('keeps a hover-opened panel open when the trigger is clicked', () => {
      const { trigger, wrapper } = renderHoverMenu();

      fireEvent.pointerEnter(wrapper, { pointerType: 'mouse' });
      fireEvent.click(trigger);

      expect(screen.getByRole('menu')).toBeInTheDocument();
    });

    it('keeps the panel open when the click arrives before the hover open renders', () => {
      const { trigger, wrapper } = renderHoverMenu();

      // A real mouse click follows the pointer's arrival within one burst of
      // input; dispatch both before React re-renders.
      act(() => {
        fireEvent.pointerEnter(wrapper, { pointerType: 'mouse' });
        fireEvent.click(trigger);
      });

      expect(screen.getByRole('menu')).toBeInTheDocument();
    });

    it('runs an item picked from a hover-opened panel', () => {
      const onSelect = vi.fn();
      const { wrapper } = renderHoverMenu(onSelect);

      fireEvent.pointerEnter(wrapper, { pointerType: 'mouse' });
      fireEvent.click(screen.getByRole('menuitem', { name: 'Close' }));

      expect(onSelect).toHaveBeenCalledTimes(1);
      expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    });

    it('ignores touch hover and keeps tap to open', () => {
      const { trigger, wrapper } = renderHoverMenu();

      fireEvent.pointerEnter(wrapper, { pointerType: 'touch' });
      expect(screen.queryByRole('menu')).not.toBeInTheDocument();

      fireEvent.click(trigger);
      expect(screen.getByRole('menu')).toBeInTheDocument();
    });

    it('does not open on hover when disabled', () => {
      render(
        <Menu
          label="Session actions"
          openOnHover
          disabled
          items={[{ label: 'Close', onSelect: vi.fn() }]}
        />,
      );
      const trigger = screen.getByRole('button', { name: 'Session actions' });

      fireEvent.pointerEnter(trigger.parentElement as HTMLElement, {
        pointerType: 'mouse',
      });

      expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    });
  });
});
