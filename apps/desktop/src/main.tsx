import "@psychevo/assets/theme.css";
import "@psychevo/components/styles.css";
import "@psychevo/workbench/styles.css";
import "@psychevo/floating/styles.css";
import React, { type ErrorInfo } from "react";
import { createRoot } from "react-dom/client";
import { App as WorkbenchApp } from "@psychevo/workbench";
import { FloatingApp } from "@psychevo/floating";
import "./desktop.css";
import {
  createDesktopFloatingRuntime,
  createDesktopWorkbenchRuntime
} from "./runtime";
import { createVisualFloatingRuntime } from "./visualRuntime";

type ErrorBoundaryState = {
  error: Error | null;
  stack: string | null;
};

class DesktopErrorBoundary extends React.Component<React.PropsWithChildren, ErrorBoundaryState> {
  override state: ErrorBoundaryState = { error: null, stack: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error, stack: null };
  }

  override componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Desktop render failed", error, info.componentStack);
    this.setState({ stack: `${error.stack ?? error.message}\n${info.componentStack}` });
  }

  override render() {
    if (this.state.error) {
      return (
        <main className="desktopError" data-error-stack={this.state.stack ?? ""} role="alert">
          Desktop render failed: {this.state.error.message}
        </main>
      );
    }
    return this.props.children;
  }
}

const params = new URLSearchParams(window.location.search);
const surface = params.get("surface") ?? "workbench";
const visualMode = params.get("visual");
document.documentElement.dataset.pevoSurface = surface;
document.body.dataset.pevoSurface = surface;

const floatingRuntime = surface === "floating"
  ? visualMode ? createVisualFloatingRuntime() : createDesktopFloatingRuntime("floating")
  : null;
const root = createRoot(document.getElementById("root")!);

root.render(
  <React.StrictMode>
    <DesktopErrorBoundary>
      {surface === "floating" ? (
        <FloatingApp runtime={floatingRuntime!} />
      ) : (
        <WorkbenchApp runtimeFactory={() => createDesktopWorkbenchRuntime("workbench")} />
      )}
    </DesktopErrorBoundary>
  </React.StrictMode>
);
