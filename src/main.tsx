import React from "react";
import ReactDOM from "react-dom/client";
import { AppProvider } from "./contexts/app.context";
import App from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import WidgetApp from "./components/widget/WidgetApp";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import "./index.css";

const windowLabel = getCurrentWebviewWindow().label;
(window as unknown as { __wbLabel: string }).__wbLabel = windowLabel;

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ErrorBoundary>
      {windowLabel === "widget" ? (
        <WidgetApp />
      ) : (
        <AppProvider>
          <App />
        </AppProvider>
      )}
    </ErrorBoundary>
  </React.StrictMode>,
);
