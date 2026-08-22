import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useApp } from "./contexts/app.context";
import ChatBar from "./components/ChatBar";
import ResponsePanel from "./components/ResponsePanel";
import Settings from "./pages/Settings";
import History from "./pages/History";
import Onboarding from "./pages/Onboarding";
import Stats from "./pages/Stats";

export default function App() {
  const { isExpanded, isOnboarded, currentPage } = useApp();

  // Debug-only issue-count listener (PLAN-03 task 4): prints counts,
  // never text. Left active while P4's widget doesn't exist yet.
  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];
    (async () => {
      try {
        const u = await listen<{ targetKey: string; issues: unknown[] }>(
          "wb://issues",
          (event) => {
            console.debug(
              `[wb] issues=${event.payload.issues.length} target=${event.payload.targetKey}`,
            );
          },
        );
        if (cancelled) { u(); return; }
        unlisteners.push(u);
      } catch {
        // Listener is diagnostic only.
      }
    })();
    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
    };
  }, []);

  // Widget orchestration (PLAN-04): the main window is the always-alive
  // coordinator — it shows/hides the lazily-created widget window.
  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];
    let zeroIssuesTimer: ReturnType<typeof setTimeout> | null = null;
    let lastRect: [number, number, number, number] | null = null;

    (async () => {
      const safe = async <T,>(
        event: Parameters<typeof listen<T>>[0],
        handler: Parameters<typeof listen<T>>[1],
      ): Promise<void> => {
        try {
          const u = await listen<T>(event, handler);
          if (cancelled) { u(); return; }
          unlisteners.push(u);
        } catch (err) {
          console.warn(`[App] listen("${String(event)}") failed:`, err);
        }
      };

      await safe<{ targetKey: string; caret: unknown; fieldRect?: [number, number, number, number] }>(
        "wb://field-focus",
        (event) => {
          if (event.payload.fieldRect) lastRect = event.payload.fieldRect;
        },
      );

      await safe<{ targetKey: string; issues: unknown[]; revoked: boolean }>(
        "wb://issues",
        async (event) => {
          // The toggle lives in config (not context) so this effect
          // stays independent of React state timing.
          let widgetEnabled = true;
          try {
            const cfg = await invoke<{ widgetEnabled: boolean }>("get_settings");
            widgetEnabled = cfg.widgetEnabled !== false;
          } catch { /* default on */ }
          const hasIssues = (event.payload.issues?.length ?? 0) > 0 && !event.payload.revoked;
          if (zeroIssuesTimer) { clearTimeout(zeroIssuesTimer); zeroIssuesTimer = null; }
          if (!widgetEnabled) return;
          if (hasIssues && lastRect) {
            try {
              await invoke("widget_show_for", { anchor: { rect: lastRect } });
            } catch { /* window manager hiccup — next tick retries */ }
          } else if (!hasIssues) {
            // Hide on zero issues after the plan's 10 s grace.
            zeroIssuesTimer = setTimeout(() => {
              invoke("widget_hide").catch(() => {});
            }, 10_000);
          }
        },
      );

      // Ctrl+Shift+W selection rewrite (hotkey event from shortcuts.rs).
      await safe("selection-rewrite", async () => {
        let hotkeyEnabled = true;
        try {
          const cfg = await invoke<{ selectionHotkeyEnabled: boolean }>("get_settings");
          hotkeyEnabled = cfg.selectionHotkeyEnabled !== false;
        } catch { /* default on */ }
        if (!hotkeyEnabled) return;
        try {
          const captured = await invoke<{ ok: boolean; text?: string; error?: string }>(
            "selection_capture",
          );
          if (!captured.ok || !captured.text) return;
          await invoke("widget_show_for", {
            anchor: lastRect ?? { rect: [200, 200, 500, 240] },
          });
          const { emit } = await import("@tauri-apps/api/event");
          await emit("widget-mode", "palette");
          await emit("palette-open", { text: captured.text });
        } catch (err) {
          console.warn("[App] selection rewrite failed:", err);
        }
      });
    })();

    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
      if (zeroIssuesTimer) clearTimeout(zeroIssuesTimer);
    };
  }, []);

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

  if (currentPage === "stats") {
    return <Stats />;
  }


  return (
    <div className="flex flex-col h-full bg-background-primary/95 backdrop-blur-md rounded-xl ring-1 ring-inset ring-zinc-800/50">
      <ChatBar />
      {isExpanded && <ResponsePanel />}
    </div>
  );
}
