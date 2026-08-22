import React from "react";
import ReactDOM from "react-dom/client";
import { AppProvider } from "./contexts/app.context";
import App from "./App";
import CursorOverlayWindow from "./components/CursorOverlayWindow";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import "./index.css";

// Detect which window we're rendering in.
// The cursor_overlay window renders only the full-screen overlay.
const windowLabel = getCurrentWebviewWindow().label;

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ErrorBoundary>
      {windowLabel === "cursor_overlay" ? (
        <CursorOverlayWindow />
      ) : (
        <AppProvider>
          <App />
        </AppProvider>
      )}
    </ErrorBoundary>
  </React.StrictMode>,
);
