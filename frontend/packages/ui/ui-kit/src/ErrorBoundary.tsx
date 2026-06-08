import { Component, type ErrorInfo, type ReactNode } from 'react';

export interface ErrorBoundaryProps {
  children: ReactNode;
  /** Rendered in place of the subtree once it throws, given the caught error. */
  fallback: (error: Error) => ReactNode;
  /**
   * When this value changes, the boundary clears the caught error and re-renders
   * the subtree. Pass something identifying the target (e.g. the focused session
   * id) so moving to a different target retries instead of staying on the
   * fallback forever.
   */
  resetKey?: unknown;
  /** Prefixes the console error so a crash is attributable to a region. */
  label?: string;
}

interface ErrorBoundaryState {
  error: Error | null;
}

/**
 * Catches render and lifecycle errors thrown by its subtree and shows a fallback
 * instead of letting the exception unmount the whole React tree. Use it to fence
 * off a non-essential region — e.g. the embedded terminal, whose attach runs in
 * an effect that can throw — so that region's failure cannot blank the rest of
 * the app. It does not catch errors from event handlers or async callbacks,
 * which React does not route through error boundaries.
 */
export class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidUpdate(prev: ErrorBoundaryProps): void {
    if (this.state.error !== null && prev.resetKey !== this.props.resetKey) {
      this.setState({ error: null });
    }
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    // The boundary stops propagation, it does not swallow the signal: keep the
    // crash (and the component stack) visible in the console for debugging.
    console.error(
      `[${this.props.label ?? 'ErrorBoundary'}]`,
      error,
      info.componentStack,
    );
  }

  render(): ReactNode {
    const { error } = this.state;
    return error !== null ? this.props.fallback(error) : this.props.children;
  }
}
