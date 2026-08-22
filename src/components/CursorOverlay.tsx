/**
 * @deprecated This component is no longer used. Cursor pointing is handled by
 * CursorOverlayWindow.tsx which renders in its own full-screen transparent
 * Tauri window for accurate screen-level coordinate mapping.
 *
 * This legacy version maps full-screen screenshot coordinates into the 600px
 * Tauri main window, making all point positions inaccurate.
 */
import { useState, useEffect, useCallback, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { MousePointer2 } from "lucide-react";
import { useApp } from "../contexts/app.context";
import type { PointTarget } from "../lib/pointParser";

interface OverlayPoint {
  x: number;
  y: number;
  label: string;
}

/** @deprecated Use CursorOverlayWindow instead */
export default function CursorOverlay() {
  const { screenshotDims } = useApp();
  const [currentPoint, setCurrentPoint] = useState<OverlayPoint | null>(null);
  const [visible, setVisible] = useState(false);
  const queueRef = useRef<OverlayPoint[]>([]);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const processingRef = useRef(false);
  // Use ref for screenshotDims so showPoint/listeners don't re-register on change
  const screenshotDimsRef = useRef(screenshotDims);
  screenshotDimsRef.current = screenshotDims;

  const clearTimer = useCallback(() => {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const dismiss = useCallback(() => {
    clearTimer();
    setVisible(false);
    setCurrentPoint(null);
    queueRef.current = [];
    processingRef.current = false;
  }, [clearTimer]);

  const processQueue = useCallback(() => {
    if (queueRef.current.length === 0) {
      processingRef.current = false;
      // Auto-dismiss after 3s on last point
      timerRef.current = setTimeout(() => {
        setVisible(false);
        setCurrentPoint(null);
      }, 3000);
      return;
    }

    processingRef.current = true;
    const next = queueRef.current.shift()!;
    setCurrentPoint(next);
    setVisible(true);

    // Delay 1s between points, then process next
    timerRef.current = setTimeout(() => {
      processQueue();
    }, 1000);
  }, []);

  const showPoint = useCallback(
    (target: PointTarget) => {
      // Map screenshot coordinates to screen-relative percentages, then to overlay pixels.
      // The overlay fills the Tauri window, so we scale to window dimensions
      // using screen proportions (screenshot captures the full screen).
      const dims = screenshotDimsRef.current;
      const screenX = dims
        ? (target.x / dims.width) * window.innerWidth
        : target.x;
      const screenY = dims
        ? (target.y / dims.height) * window.innerHeight
        : target.y;
      // NOTE: Coordinates are proportionally correct but the overlay only
      // covers the Tauri window (600px), not the full screen. A future version
      // should use a separate full-screen transparent Tauri window for accurate
      // screen-level pointing.

      const point: OverlayPoint = {
        x: screenX,
        y: screenY,
        label: target.label,
      };

      clearTimer();

      if (!processingRef.current) {
        // Start showing immediately
        setCurrentPoint(point);
        setVisible(true);
        processingRef.current = true;

        // Auto-dismiss after 3s if no more points in queue
        timerRef.current = setTimeout(() => {
          processQueue();
        }, 3000);
      } else {
        // Queue it
        queueRef.current.push(point);
      }
    },
    [clearTimer, processQueue],
  );

  // Listen for pointer_show and pointer_hide Tauri events
  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];

    (async () => {
      const u1 = await listen<PointTarget>("pointer_show", (event) => {
        showPoint(event.payload);
      });
      if (cancelled) { u1(); return; }
      unlisteners.push(u1);

      const u2 = await listen("pointer_hide", () => {
        dismiss();
      });
      if (cancelled) { u2(); return; }
      unlisteners.push(u2);
    })();

    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
    };
  }, [showPoint, dismiss]);

  // Escape key to dismiss
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && visible) {
        dismiss();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [visible, dismiss]);

  // Cleanup timers on unmount
  useEffect(() => {
    return () => clearTimer();
  }, [clearTimer]);

  if (!visible || !currentPoint) return null;

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        pointerEvents: "none",
        zIndex: 9999,
      }}
    >
      <div
        style={{
          position: "absolute",
          left: currentPoint.x,
          top: currentPoint.y,
          transition: "transform 0.6s cubic-bezier(0.34, 1.56, 0.64, 1)",
          transform: "translate(-12px, -12px)",
        }}
      >
        <MousePointer2
          size={24}
          className="text-blue-400 drop-shadow-lg"
          fill="rgba(96, 165, 250, 0.3)"
        />
        {currentPoint.label && (
          <div className="absolute left-7 top-0 bg-zinc-900/90 text-white text-xs px-2 py-1 rounded whitespace-nowrap shadow-lg">
            {currentPoint.label}
          </div>
        )}
      </div>
    </div>
  );
}
