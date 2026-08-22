/**
 * Full-screen cursor overlay rendered in the `cursor_overlay` Tauri window.
 * Transparent, frameless, always-on-top, click-through.
 *
 * Features:
 * - Spring-physics cursor animation (damped harmonic oscillator)
 * - SVG mask spotlight overlay (dims screen, bright cutout at target)
 * - Trail ghosts for motion feedback
 * - Queue-based sequential point processing
 */
import { useState, useEffect, useCallback, useRef } from "react";
import { listen, emit } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { MousePointer2 } from "lucide-react";
import { SpringValue } from "../lib/springPhysics";

const debugLog = (msg: string) => invoke("debug_log", { message: msg }).catch(() => {});

interface PointTarget {
  x: number;
  y: number;
  label: string;
  screen: number;
}

interface ScreenDims {
  width: number;
  height: number;
}

interface OverlayPoint {
  x: number;
  y: number;
  label: string;
}

// Trail ghost: previous cursor positions for motion feedback
const TRAIL_LENGTH = 3;

export default function CursorOverlayWindow() {
  const [visible, setVisible] = useState(false);
  const [label, setLabel] = useState("");
  const [posState, setPosState] = useState({ x: 0, y: 0 });
  const [spotlightOpacity, setSpotlightOpacity] = useState(0);
  const [trail, setTrail] = useState<Array<{ x: number; y: number }>>([]);

  const queueRef = useRef<OverlayPoint[]>([]);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const processingRef = useRef(false);
  const screenshotDimsRef = useRef<ScreenDims | null>(null);
  const springXRef = useRef(new SpringValue(0));
  const springYRef = useRef(new SpringValue(0));
  const rafRef = useRef<number | null>(null);
  const lastTimeRef = useRef(0);
  const trailBufferRef = useRef<Array<{ x: number; y: number }>>([]);
  const trailCounterRef = useRef(0);

  const clearTimer = useCallback(() => {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const stopAnimation = useCallback(() => {
    if (rafRef.current) {
      cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
  }, []);

  const dismiss = useCallback(() => {
    clearTimer();
    stopAnimation();
    setSpotlightOpacity(0);
    // Fade out, then clear state. No hide_pointer call — the Tauri
    // window stays visible (but renders nothing). This avoids losing
    // WS_EX_TRANSPARENT (click-through) on Windows during hide/show.
    setTimeout(() => {
      setVisible(false);
      setLabel("");
      queueRef.current = [];
      processingRef.current = false;
    }, 300);
  }, [clearTimer, stopAnimation]);

  // rAF animation loop driven by spring physics
  const startAnimation = useCallback((targetX: number, targetY: number) => {
    springXRef.current.setTarget(targetX);
    springYRef.current.setTarget(targetY);

    if (rafRef.current) return; // Already running

    lastTimeRef.current = performance.now();

    const animate = (now: number) => {
      // Windows throttles rAF for transparent/unfocused WebView2 windows
      // (sometimes to 1fps). Simulate the full elapsed gap in 16ms chunks
      // so the cursor catches up even at low frame rates.
      // Clamp to >= 0: rAF timestamps can be slightly older than
      // performance.now() (locked to frame start), causing negative dt
      // which would skip the while loop entirely.
      let remainingDt = Math.max(0, Math.min((now - lastTimeRef.current) / 1000, 1.0));
      lastTimeRef.current = now;

      while (remainingDt > 0) {
        const stepDt = Math.min(remainingDt, 0.016);
        remainingDt -= stepDt;
        springXRef.current.step(stepDt);
        springYRef.current.step(stepDt);
      }

      const x = springXRef.current.position;
      const y = springYRef.current.position;

      setPosState({ x, y });

      // Use isSettled() to decide if animation should continue — NOT the
      // step() return value. If dt was 0 on the first frame (rAF timing
      // edge case), step() returns false but the spring hasn't reached
      // its target. isSettled() checks actual position vs target.
      const isMoving = !springXRef.current.isSettled() || !springYRef.current.isSettled();

      // Update trail buffer every 3 frames
      trailCounterRef.current++;
      if (trailCounterRef.current % 3 === 0 && isMoving) {
        trailBufferRef.current.push({ x, y });
        if (trailBufferRef.current.length > TRAIL_LENGTH) {
          trailBufferRef.current.shift();
        }
        setTrail([...trailBufferRef.current]);
      }

      if (isMoving) {
        rafRef.current = requestAnimationFrame(animate);
      } else {
        rafRef.current = null;
        // Clear trail when settled
        trailBufferRef.current = [];
        setTrail([]);
      }
    };

    rafRef.current = requestAnimationFrame(animate);
  }, []);

  const processQueue = useCallback(() => {
    if (queueRef.current.length === 0) {
      processingRef.current = false;
      timerRef.current = setTimeout(() => {
        dismiss();
      }, 3000);
      return;
    }

    processingRef.current = true;
    const next = queueRef.current.shift()!;
    setLabel(next.label);
    startAnimation(next.x, next.y);

    timerRef.current = setTimeout(() => {
      processQueue();
    }, 2500);
  }, [dismiss, startAnimation]);

  const showPoint = useCallback(
    (target: PointTarget) => {
      const dims = screenshotDimsRef.current;
      const screenX = dims
        ? (target.x / dims.width) * window.innerWidth
        : target.x;
      const screenY = dims
        ? (target.y / dims.height) * window.innerHeight
        : target.y;

      // Debug: log coordinate mapping to stderr for pointing accuracy diagnosis
      debugLog(`overlay: point(${target.x},${target.y}) → css(${screenX.toFixed(0)},${screenY.toFixed(0)}) dims=${dims?.width}x${dims?.height} win=${window.innerWidth}x${window.innerHeight} dpr=${window.devicePixelRatio} queued=${processingRef.current}`);

      const point: OverlayPoint = { x: screenX, y: screenY, label: target.label };

      if (!processingRef.current) {
        // First point — cancel any lingering dismiss timer AND any
        // pending requestAnimationFrame from a prior point's spring,
        // since reassigning the spring refs without stopping animation
        // first leaves the rAF reading the new ref with a stale dt
        // (causing a brief jitter on rapid-fire pointing).
        clearTimer();
        stopAnimation();
        springXRef.current = new SpringValue(screenX);
        springYRef.current = new SpringValue(screenY);
        setPosState({ x: screenX, y: screenY });
        setLabel(point.label);
        setVisible(true);
        setSpotlightOpacity(0.45);
        processingRef.current = true;

        timerRef.current = setTimeout(() => {
          processQueue();
        }, 3000);
      } else {
        // Subsequent points — queue them. DON'T clear the existing timer;
        // it will call processQueue() which drains the queue. If we cleared
        // it, the queue would never be processed and the overlay would stick.
        queueRef.current.push(point);
      }
    },
    [clearTimer, processQueue, startAnimation, stopAnimation],
  );

  // Listen for events
  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];

    (async () => {
      const u0 = await listen<ScreenDims>("screenshot_dims", (event) => {
        screenshotDimsRef.current = event.payload;
      });
      if (cancelled) { u0(); return; }
      unlisteners.push(u0);

      const u1 = await listen<PointTarget>("pointer_show", (event) => {
        showPoint(event.payload);
      });
      if (cancelled) { u1(); return; }
      unlisteners.push(u1);

      const u2 = await listen("pointer_hide", () => {
        clearTimer();
        stopAnimation();
        setVisible(false);
        setLabel("");
        setSpotlightOpacity(0);
        queueRef.current = [];
        processingRef.current = false;
      });
      if (cancelled) { u2(); return; }
      unlisteners.push(u2);

      // Listeners are now active — signal the main window so it can
      // re-emit its last screenshot dimensions. Covers the cold-start
      // race where the overlay webview mounts AFTER `screenshot_dims`
      // was already broadcast from the main window.
      emit("overlay_ready").catch(() => {});
    })();

    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
    };
  }, [showPoint, clearTimer, stopAnimation]);

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

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      clearTimer();
      stopAnimation();
    };
  }, [clearTimer, stopAnimation]);

  if (!visible) return null;

  const spotlightRadius = 50;

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        pointerEvents: "none",
        background: "transparent",
      }}
    >
      {/* SVG spotlight overlay: dims screen with bright cutout at target */}
      <svg
        width="100%"
        height="100%"
        style={{
          position: "fixed",
          inset: 0,
          zIndex: 9998,
          pointerEvents: "none",
          transition: "opacity 0.3s ease",
          opacity: spotlightOpacity > 0 ? 1 : 0,
        }}
      >
        <defs>
          <mask id="spotlight-mask">
            <rect width="100%" height="100%" fill="white" />
            <ellipse
              cx={posState.x}
              cy={posState.y}
              rx={spotlightRadius}
              ry={spotlightRadius}
              fill="black"
            />
          </mask>
          <radialGradient id="spotlight-glow">
            <stop offset="0%" stopColor="rgba(96,165,250,0.15)" />
            <stop offset="100%" stopColor="rgba(96,165,250,0)" />
          </radialGradient>
        </defs>
        {/* Dark overlay with cutout */}
        <rect
          width="100%"
          height="100%"
          fill={`rgba(0,0,0,${spotlightOpacity})`}
          mask="url(#spotlight-mask)"
        />
        {/* Glow around spotlight edge */}
        <ellipse
          cx={posState.x}
          cy={posState.y}
          rx={spotlightRadius + 20}
          ry={spotlightRadius + 20}
          fill="url(#spotlight-glow)"
        />
        {/* Pulse ring */}
        <ellipse
          cx={posState.x}
          cy={posState.y}
          rx={spotlightRadius}
          ry={spotlightRadius}
          fill="none"
          stroke="rgba(96,165,250,0.3)"
          strokeWidth="2"
        >
          <animate
            attributeName="rx"
            values={`${spotlightRadius};${spotlightRadius + 8};${spotlightRadius}`}
            dur="2s"
            repeatCount="indefinite"
          />
          <animate
            attributeName="ry"
            values={`${spotlightRadius};${spotlightRadius + 8};${spotlightRadius}`}
            dur="2s"
            repeatCount="indefinite"
          />
          <animate
            attributeName="opacity"
            values="0.4;0.1;0.4"
            dur="2s"
            repeatCount="indefinite"
          />
        </ellipse>
      </svg>

      {/* Trail ghosts */}
      {trail.map((t, i) => (
        <div
          key={i}
          style={{
            position: "absolute",
            left: t.x,
            top: t.y,
            transform: "translate(-12px, -12px)",
            opacity: 0.1 + (i / TRAIL_LENGTH) * 0.15,
            zIndex: 9999,
          }}
        >
          <MousePointer2
            size={24}
            className="text-blue-400"
            fill="rgba(96, 165, 250, 0.15)"
          />
        </div>
      ))}

      {/* Main cursor */}
      <div
        style={{
          position: "absolute",
          left: posState.x,
          top: posState.y,
          transform: "translate(-12px, -12px)",
          zIndex: 10000,
        }}
      >
        <MousePointer2
          size={28}
          className="text-blue-400 drop-shadow-lg"
          fill="rgba(96, 165, 250, 0.3)"
          style={{ filter: "drop-shadow(0 0 8px rgba(96, 165, 250, 0.5))" }}
        />
        {label && (
          <div
            className={`absolute bg-zinc-900/95 text-white text-sm px-3 py-1.5 rounded-lg whitespace-nowrap shadow-xl border border-zinc-700/50 ${
              posState.x > window.innerWidth - 200 ? "right-8" : "left-8"
            } ${
              posState.y > window.innerHeight - 60 ? "bottom-8" : "top-0"
            }`}
            style={{ filter: "drop-shadow(0 2px 8px rgba(0,0,0,0.5))" }}
          >
            {label}
          </div>
        )}
      </div>
    </div>
  );
}
