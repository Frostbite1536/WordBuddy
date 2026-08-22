import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useApp } from "./contexts/app.context";
import ChatBar from "./components/ChatBar";
import ResponsePanel from "./components/ResponsePanel";
import Settings from "./pages/Settings";
import History from "./pages/History";
import Onboarding from "./pages/Onboarding";

export default function App() {
  const { isExpanded, isOnboarded, currentPage } = useApp();

  // Global shortcut: toggle visibility
  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];

    (async () => {
      const u = await listen("toggle-visibility", () => {
        invoke("toggle_visibility").catch(() => {});
      });
      if (cancelled) { u(); return; }
      unlisteners.push(u);
    })();

    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
    };
  }, []);

  if (!isOnboarded) {
    return <Onboarding />;
  }

  if (currentPage === "settings") {
    return <Settings />;
  }

  if (currentPage === "history") {
    return <History />;
  }


  return (
    <div className="flex flex-col h-full bg-background-primary/95 backdrop-blur-md rounded-xl ring-1 ring-inset ring-zinc-800/50">
      <ChatBar />
      {isExpanded && <ResponsePanel />}
    </div>
  );
}
