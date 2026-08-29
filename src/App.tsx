import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emitTo, listen } from "@tauri-apps/api/event";
import { useApp } from "./contexts/app.context";
import ChatBar from "./components/ChatBar";
import ResponsePanel from "./components/ResponsePanel";
import Settings from "./pages/Settings";
import History from "./pages/History";
import Onboarding from "./pages/Onboarding";
import Stats from "./pages/Stats";

type WidgetMode = "card" | "palette";
type WidgetIssuesPayload = {
  targetKey: string;
  issues: unknown[];
  revoked: boolean;
};
type WidgetFieldTextPayload = { targetKey: string; text?: string };

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
    let lastFocusedTargetKey: string | null = null;
    // Do not show a card until the first config read completes. This avoids
    // a focus event briefly re-enabling a card the user has disabled.
    let widgetEnabled = false;
    let issuesEpoch = 0;
    let widgetReady = false;
    let widgetMode: WidgetMode | null = null;
    let widgetTaskChain: Promise<void> = Promise.resolve();
    const widgetReadyWaiters = new Set<() => void>();
    const widgetModeWaiters = new Map<WidgetMode, Set<() => void>>();
    // Keep only the active snapshot. Field text is privacy-sensitive and must
    // not accumulate in a long-lived per-target cache.
    let latestIssues: WidgetIssuesPayload | null = null;
    let latestFieldText: WidgetFieldTextPayload | null = null;
    // Dismissal memory: closing the card means "not now" for THIS set
    // of mistakes. Signature is offset-free ((ruleId|original) pairs)
    // so fixing one typo doesn't shift spans into a "new" signature
    // and resurrect the window for the remaining ones. A genuinely
    // different issue set re-shows immediately.
    const sigByTarget = new Map<string, string>();
    const dismissedSigByTarget = new Map<string, string>();
    const hasIssuesByTarget = new Map<string, boolean>();
    const issueSig = (
      issues: Array<{ ruleId?: unknown; original?: unknown }>,
    ): string =>
      JSON.stringify(
        issues
          .map((i) => [String(i.ruleId ?? ""), String(i.original ?? "")])
          .sort(([leftRule, leftOriginal], [rightRule, rightOriginal]) =>
            leftRule.localeCompare(rightRule) || leftOriginal.localeCompare(rightOriginal),
          ),
      );

    const waitForWidgetReady = (): Promise<void> => {
      if (widgetReady) return Promise.resolve();
      return new Promise((resolve, reject) => {
        const done = () => {
          clearTimeout(timeout);
          widgetReadyWaiters.delete(done);
          resolve();
        };
        const timeout = window.setTimeout(() => {
          widgetReadyWaiters.delete(done);
          reject(new Error("widget did not become ready"));
        }, 3_000);
        widgetReadyWaiters.add(done);
      });
    };

    const waitForWidgetMode = (mode: WidgetMode): Promise<void> => {
      if (widgetMode === mode) return Promise.resolve();
      return new Promise((resolve, reject) => {
        const done = () => {
          clearTimeout(timeout);
          widgetModeWaiters.get(mode)?.delete(done);
          resolve();
        };
        const timeout = window.setTimeout(() => {
          widgetModeWaiters.get(mode)?.delete(done);
          reject(new Error(`widget mode ${mode} did not become ready`));
        }, 3_000);
        const waiters = widgetModeWaiters.get(mode) ?? new Set<() => void>();
        waiters.add(done);
        widgetModeWaiters.set(mode, waiters);
      });
    };

    const prepareWidget = async (
      mode: WidgetMode,
      rect: [number, number, number, number],
    ): Promise<void> => {
      await invoke("widget_show_for", { anchor: { rect } });
      if (!widgetReady) {
        const ready = waitForWidgetReady();
        // Covers an already-created hidden widget (for example after a main
        // webview reload). A newly-created widget also emits readiness on
        // mount, so losing this early request is harmless.
        await emitTo("widget", "wb://widget-ready-request").catch(() => {});
        await ready;
      }
      if (widgetMode !== mode) {
        const ready = waitForWidgetMode(mode);
        await emitTo("widget", "widget-mode", mode);
        await ready;
      }
    };

    const queueWidgetTask = <T,>(task: () => Promise<T>): Promise<T> => {
      const run = widgetTaskChain.then(task, task);
      widgetTaskChain = run.then(
        () => undefined,
        () => undefined,
      );
      return run;
    };

    const showCardFor = async (
      targetKey: string,
      rect: [number, number, number, number],
    ): Promise<void> =>
      queueWidgetTask(async () => {
        if (
          cancelled ||
          lastFocusedTargetKey !== targetKey ||
          latestIssues?.targetKey !== targetKey ||
          !hasIssuesByTarget.get(targetKey)
        ) {
          return;
        }
        await prepareWidget("card", rect);
        const issuesPayload = latestIssues?.targetKey === targetKey
          ? latestIssues
          : null;
        if (issuesPayload) {
          await emitTo("widget", "wb://issues", issuesPayload);
        }
        const fieldTextPayload = latestFieldText?.targetKey === targetKey
          ? latestFieldText
          : null;
        if (fieldTextPayload) {
          await emitTo("widget", "wb://field-text", fieldTextPayload);
        }
      });

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

      await safe<{ mode?: WidgetMode }>("wb://widget-ready", (event) => {
        widgetReady = true;
        if (event.payload?.mode === "card" || event.payload?.mode === "palette") {
          widgetMode = event.payload.mode;
        }
        widgetReadyWaiters.forEach((resolve) => resolve());
      });

      await safe<{ mode: WidgetMode }>("wb://widget-mode-ready", (event) => {
        widgetMode = event.payload.mode;
        widgetModeWaiters.get(widgetMode)?.forEach((resolve) => resolve());
      });

      await safe<WidgetFieldTextPayload>("wb://field-text", (event) => {
        if (typeof event.payload.text === "string") {
          latestFieldText = event.payload;
        }
      });

      await safe<{ targetKey: string; caret: unknown; fieldRect?: [number, number, number, number] }>(
        "wb://field-focus",
        (event) => {
          if (event.payload.fieldRect) lastRect = event.payload.fieldRect;
          // Follow the field: if the card is legitimately visible for
          // this target (not dismissed), dock to the field's new rect.
          // widget_show_for is idempotent show + move.
          const key = event.payload.targetKey;
          lastFocusedTargetKey = key;
          const sig = sigByTarget.get(key);
          if (
            widgetEnabled &&
            lastRect &&
            sig &&
            hasIssuesByTarget.get(key) &&
            sig !== dismissedSigByTarget.get(key)
          ) {
            void showCardFor(key, lastRect).catch(() => {});
          }
        },
      );

      await safe<WidgetIssuesPayload>(
        "wb://issues",
        async (event) => {
          const epoch = ++issuesEpoch;
          latestIssues = event.payload;
          if (latestFieldText?.targetKey !== event.payload.targetKey) {
            latestFieldText = null;
          }
          // The toggle lives in config (not context) so this effect
          // stays independent of React state timing.
          let nextWidgetEnabled = true;
          try {
            const cfg = await invoke<{ widget_enabled: boolean }>("get_settings");
            // AppConfig serializes snake_case (no serde rename); camelCase
            // reads were always undefined, forcing the widget on.
            nextWidgetEnabled = cfg.widget_enabled !== false;
          } catch { /* default on */ }
          // get_settings is asynchronous. A later issue event can complete
          // first, so never let an older event show/hide the wrong card.
          if (cancelled || epoch !== issuesEpoch) return;
          widgetEnabled = nextWidgetEnabled;
          const issues = Array.isArray(event.payload.issues)
            ? (event.payload.issues as Array<{ ruleId?: unknown; original?: unknown }>)
            : [];
          const sig = issueSig(issues);
          sigByTarget.set(event.payload.targetKey, sig);
          const hasIssues = issues.length > 0 && !event.payload.revoked;
          if (!hasIssues) latestFieldText = null;
          hasIssuesByTarget.set(event.payload.targetKey, hasIssues);
          if (zeroIssuesTimer) { clearTimeout(zeroIssuesTimer); zeroIssuesTimer = null; }
          if (!widgetEnabled) return;
          if (
            hasIssues &&
            lastRect &&
            lastFocusedTargetKey === event.payload.targetKey &&
            sig !== dismissedSigByTarget.get(event.payload.targetKey)
          ) {
            try {
              await showCardFor(event.payload.targetKey, lastRect);
            } catch { /* window manager hiccup — next tick retries */ }
          } else if (!hasIssues) {
            // Hide on zero issues after the plan's 10 s grace.
            const targetKey = event.payload.targetKey;
            zeroIssuesTimer = setTimeout(() => {
              if (
                lastFocusedTargetKey === targetKey &&
                !hasIssuesByTarget.get(targetKey) &&
                widgetMode === "card"
              ) {
                invoke("widget_hide").catch(() => {});
              }
            }, 10_000);
          }
        },
      );

      await safe<{ targetKey: string }>("wb://widget-dismissed", (event) => {
        const sig = sigByTarget.get(event.payload.targetKey);
        if (sig) dismissedSigByTarget.set(event.payload.targetKey, sig);
        if (latestFieldText?.targetKey === event.payload.targetKey) {
          latestFieldText = null;
        }
      });

      // Ctrl+Shift+W selection rewrite (hotkey event from shortcuts.rs).
      await safe("selection-rewrite", async () => {
        let hotkeyEnabled = true;
        try {
          const cfg = await invoke<{ selection_hotkey_enabled: boolean }>("get_settings");
          hotkeyEnabled = cfg.selection_hotkey_enabled !== false;
        } catch { /* default on */ }
        if (!hotkeyEnabled) return;
        try {
          const captured = await invoke<{ ok: boolean; text?: string; error?: string }>(
            "selection_capture",
          );
          if (!captured.ok || !captured.text) return;
          await queueWidgetTask(async () => {
            await prepareWidget(
              "palette",
              lastRect ?? [200, 200, 500, 240],
            );
            await emitTo("widget", "palette-open", { text: captured.text });
          });
        } catch (err) {
          console.warn("[App] selection rewrite failed:", err);
        }
      });
    })();

    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
      widgetReadyWaiters.clear();
      widgetModeWaiters.clear();
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
