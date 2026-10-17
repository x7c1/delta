import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { Meter } from './Meter';

describe('Meter', () => {
  it('renders a track+fill structure whose fill width matches the value', () => {
    render(<Meter value={42} title="context usage" />);

    const meter = screen.getByRole('meter', { name: 'context usage' });
    expect(meter).toHaveAttribute('aria-valuenow', '42');
    expect(meter).toHaveAttribute('aria-valuemin', '0');
    expect(meter).toHaveAttribute('aria-valuemax', '100');

    // The fill is a child div whose width is the percentage.
    const fill = meter.firstElementChild as HTMLElement;
    expect(fill).toBeTruthy();
    expect(fill.style.width).toBe('42%');
  });

  it('clamps a value above 100 to 100', () => {
    render(<Meter value={150} title="over" />);

    const meter = screen.getByRole('meter', { name: 'over' });
    expect(meter).toHaveAttribute('aria-valuenow', '100');
    expect((meter.firstElementChild as HTMLElement).style.width).toBe('100%');
  });

  it('clamps a negative value to 0', () => {
    render(<Meter value={-20} title="under" />);

    const meter = screen.getByRole('meter', { name: 'under' });
    expect(meter).toHaveAttribute('aria-valuenow', '0');
    expect((meter.firstElementChild as HTMLElement).style.width).toBe('0%');
  });

  it('treats a non-finite value as 0', () => {
    render(<Meter value={NaN} title="nan" />);

    const meter = screen.getByRole('meter', { name: 'nan' });
    expect(meter).toHaveAttribute('aria-valuenow', '0');
    expect((meter.firstElementChild as HTMLElement).style.width).toBe('0%');
  });

  it('announces the caller readout as aria-valuetext, keeping aria-valuenow numeric', () => {
    // The caller renders `43%` beside a 42.7 value; the screen reader must
    // announce that same text, not the raw 42.7.
    render(<Meter value={42.7} title="usage" valueText="43%" />);

    const meter = screen.getByRole('meter', { name: 'usage' });
    expect(meter).toHaveAttribute('aria-valuetext', '43%');
    expect(meter).toHaveAttribute('aria-valuenow', '42.7');
  });

  it('sets no aria-valuetext when the caller gives none', () => {
    render(<Meter value={42.7} title="bare" />);

    expect(screen.getByRole('meter', { name: 'bare' })).not.toHaveAttribute(
      'aria-valuetext',
    );
  });

  it('takes an accessible name without a native tooltip via label', () => {
    render(<Meter value={10} label="quiet" />);

    const meter = screen.getByRole('meter', { name: 'quiet' });
    expect(meter).not.toHaveAttribute('title');
  });
});
