import "@psychevo/assets/theme.css";
import "@psychevo/components/styles.css";
import React, { type ErrorInfo } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./app.css";

type ErrorBoundaryState = {
  error: Error | null;
  stack: string | null;
};

class WorkbenchErrorBoundary extends React.Component<React.PropsWithChildren, ErrorBoundaryState> {
  override state: ErrorBoundaryState = { error: null, stack: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error, stack: null };
  }

  override componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Workbench render failed", error, info.componentStack);
    this.setState({ stack: `${error.stack ?? error.message}\n${info.componentStack}` });
  }

  override render() {
    if (this.state.error) {
      return (
        <main className="appShell">
          <header className="topBar">
            <div className="brandMark">
              <span className="brandGlyph">p</span>
              <div>
                <h1>pevo</h1>
                <p>workbench</p>
              </div>
            </div>
          </header>
          <div className="errorBand" data-error-stack={this.state.stack ?? ""} role="alert">
            Workbench render failed: {this.state.error.message}
          </div>
        </main>
      );
    }

    return this.props.children;
  }
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <WorkbenchErrorBoundary>
      <App />
    </WorkbenchErrorBoundary>
  </React.StrictMode>
);
