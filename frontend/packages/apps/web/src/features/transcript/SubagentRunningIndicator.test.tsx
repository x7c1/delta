import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import type { SubagentActivity } from '../../store/liveStore';
import { SubagentRunningIndicator } from './SubagentRunningIndicator';

function subagent(overrides: Partial<SubagentActivity> = {}): SubagentActivity {
  return {
    toolUseId: 'toolu_a1',
    subagentType: 'general-purpose',
    description: 'Probe the codebase',
    ...overrides,
  };
}

describe('SubagentRunningIndicator', () => {
  it('renders nothing when no subagent is running', () => {
    const { container } = render(<SubagentRunningIndicator subagents={[]} />);

    expect(
      screen.queryByTestId('subagent-running-indicator'),
    ).not.toBeInTheDocument();
    expect(container).toBeEmptyDOMElement();
  });

  it('renders a singular heading and the description for one subagent', () => {
    render(<SubagentRunningIndicator subagents={[subagent()]} />);

    const indicator = screen.getByTestId('subagent-running-indicator');
    expect(indicator).toHaveTextContent('Subagent running');
    expect(indicator).toHaveTextContent('Probe the codebase');
  });

  it('falls back to the subagent type when no description is given', () => {
    render(
      <SubagentRunningIndicator
        subagents={[subagent({ description: null })]}
      />,
    );

    expect(
      screen.getByTestId('subagent-running-indicator'),
    ).toHaveTextContent('general-purpose');
  });

  it('lists each of multiple concurrent subagents with a count heading', () => {
    render(
      <SubagentRunningIndicator
        subagents={[
          subagent({ toolUseId: 'toolu_a1', description: 'First task' }),
          subagent({ toolUseId: 'toolu_a2', description: 'Second task' }),
        ]}
      />,
    );

    const indicator = screen.getByTestId('subagent-running-indicator');
    expect(indicator).toHaveTextContent('2 subagents running');
    expect(indicator).toHaveTextContent('First task');
    expect(indicator).toHaveTextContent('Second task');
  });
});
