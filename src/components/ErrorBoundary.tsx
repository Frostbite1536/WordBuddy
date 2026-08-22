import React from "react";

interface Props {
  children: React.ReactNode;
}

interface State {
  error: Error | null;
  componentStack: string | null;
}

export class ErrorBoundary extends React.Component<Props, State> {
  state: State = { error: null, componentStack: null };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error("[ErrorBoundary] render error:", error, info.componentStack);
    // Surface the component stack in state too — without it, a user
    // bug report contains only error.message, which is rarely enough
    // to localize the throw.
    this.setState({ componentStack: info.componentStack ?? null });
  }

  private handleReset = () =>
    this.setState({ error: null, componentStack: null });

  render() {
    const { error, componentStack } = this.state;
    if (!error) return this.props.children;

    return (
      <div
        role="alert"
        className="flex h-full w-full flex-col items-center justify-center gap-3 bg-zinc-950 p-6 text-zinc-100"
      >
        <div className="text-sm font-semibold text-red-400">
          WorkBuddy hit an unexpected error.
        </div>
        <pre className="max-h-48 max-w-full overflow-auto rounded bg-zinc-900 p-3 text-xs text-zinc-300">
          {error.message}
        </pre>
        {componentStack && (
          <details className="max-w-full text-xs text-zinc-400">
            <summary className="cursor-pointer text-zinc-500 hover:text-zinc-300">
              Component stack (for bug reports)
            </summary>
            <pre className="mt-2 max-h-48 overflow-auto rounded bg-zinc-900 p-3 text-[11px] text-zinc-400">
              {componentStack.trim()}
            </pre>
          </details>
        )}
        <button
          onClick={this.handleReset}
          className="rounded bg-zinc-800 px-3 py-1 text-xs text-zinc-100 hover:bg-zinc-700"
        >
          Try again
        </button>
      </div>
    );
  }
}
