import React, { createContext, useContext, useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Settings {
  api_keys: Record<string, string>;
  provider: string;
  model: string;
  auto_screenshot: boolean;
  tts_enabled: boolean;
  tts_voice: string;
  tts_provider: string;
  stt_provider: string;
  theme: string;
  tutor_mode: boolean;
  capture_monitor: string;
  cursor_overlay_enabled: boolean;
  a11y_detection_enabled: boolean;
  mask_form_inputs: boolean;
  extension_highlight_enabled: boolean;
  wotch_integration_enabled: boolean;
  recorder_enabled: boolean;
  recorder_interval_secs: number;
  recorder_retention_days: number;
  journal_capture_monitor: string;
  analysis_provider: string;
  analysis_model: string;
}

interface WindowContext {
  title: string;
}

export interface Message {
  id: string;
  role: "user" | "assistant";
  content: string;
  screenshot?: string;
  timestamp: number;
}

export interface ScreenshotDimensions {
  width: number;
  height: number;
}

// Pending external question awaiting user confirmation. Set by the
// /ask endpoint via the `external-question` event; cleared when the
// user clicks Submit (which calls handleSubmit) or Discard. This
// banner-based confirmation is the user-facing gate against the S1
// audit threat model: any local process holding the extension token
// could otherwise fire LLM prompts at the user's API quota silently.
export interface ExternalQuestionPending {
  source: string;
  question: string;
  context?: string;
  // Wall-clock when received, used to expire stale prompts (a queued
  // /ask from before the user closed and reopened WorkBuddy).
  receivedAt: number;
}

// Journal day attached to the next chat turns ("Ask about this day").
// Set by the Journal page, consumed by ChatBar's system-prompt builder,
// cleared via the chip in the bar.
export interface JournalChatContext {
  day: string;
  text: string;
}

interface AppState {
  settings: Settings;
  updateSettings: (s: Partial<Settings>) => void;
  isExpanded: boolean;
  setIsExpanded: (v: boolean) => void;
  currentContext: WindowContext | null;
  messages: Message[];
  setMessages: React.Dispatch<React.SetStateAction<Message[]>>;
  addMessage: (msg: Message) => void;
  clearMessages: () => void;
  isStreaming: boolean;
  setIsStreaming: (v: boolean) => void;
  isOnboarded: boolean;
  setIsOnboarded: (v: boolean) => void;
  currentPage: "chat" | "settings" | "history" | "onboarding" | "journal";
  setCurrentPage: (p: "chat" | "settings" | "history" | "onboarding" | "journal") => void;
  screenshotDims: ScreenshotDimensions | null;
  setScreenshotDims: (d: ScreenshotDimensions | null) => void;
  conversationIdRef: React.MutableRefObject<string | null>;
  externalQuestion: ExternalQuestionPending | null;
  setExternalQuestion: (v: ExternalQuestionPending | null) => void;
  journalChatContext: JournalChatContext | null;
  setJournalChatContext: (v: JournalChatContext | null) => void;
  // ChatBar populates this with its handleSubmit so ResponsePanel
  // (where the externalQuestion banner lives) can fire the prompt
  // without ChatBar having to render the banner itself. Mirror of
  // the existing handleSubmitRef pattern inside ChatBar.
  submitExternalRef: React.MutableRefObject<(composed: string) => void>;
}

const defaultSettings: Settings = {
  api_keys: {},
  provider: "anthropic",
  model: "claude-sonnet-4-20250514",
  auto_screenshot: true,
  tts_enabled: false,
  tts_voice: "default",
  tts_provider: "elevenlabs",
  stt_provider: "whisper",
  theme: "dark",
  tutor_mode: false,
  capture_monitor: "auto",
  cursor_overlay_enabled: false,
  a11y_detection_enabled: true,
  mask_form_inputs: false,
  extension_highlight_enabled: true,
  wotch_integration_enabled: true,
  recorder_enabled: false,
  recorder_interval_secs: 10,
  recorder_retention_days: 14,
  journal_capture_monitor: "",
  analysis_provider: "",
  analysis_model: "",
};

const AppContext = createContext<AppState | null>(null);

export function AppProvider({ children }: { children: React.ReactNode }) {
  const [settings, setSettings] = useState<Settings>(defaultSettings);
  const [isExpanded, setIsExpanded] = useState(false);
  const [currentContext, setCurrentContext] = useState<WindowContext | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [isStreaming, setIsStreaming] = useState(false);
  const [isOnboarded, setIsOnboarded] = useState(false);
  const [currentPage, setCurrentPage] = useState<"chat" | "settings" | "history" | "onboarding" | "journal">("chat");
  const [screenshotDims, setScreenshotDims] = useState<ScreenshotDimensions | null>(null);
  const [externalQuestion, setExternalQuestion] = useState<ExternalQuestionPending | null>(null);
  const [journalChatContext, setJournalChatContext] = useState<JournalChatContext | null>(null);
  const submitExternalRef = useRef<(composed: string) => void>(() => {});
  const conversationIdRef = useRef<string | null>(null);
  // Ref for isStreaming — used by detect_active_window poll to skip during streaming.
  // Updated in an effect rather than during render so React 19's concurrent
  // renderer can't strand the ref on a discarded render.
  const isStreamingRef = useRef(false);
  useEffect(() => {
    isStreamingRef.current = isStreaming;
  }, [isStreaming]);

  // Load settings on mount
  useEffect(() => {
    invoke<Settings>("get_settings")
      .then((s) => {
        setSettings(s);
        // Onboarded if any LLM key is configured, or provider is Ollama (no key needed)
        const hasAnyKey = Object.values(s.api_keys || {}).some((k) => k && k.length > 0);
        setIsOnboarded(hasAnyKey || s.provider === "ollama");
      })
      .catch(() => {
        // Settings load failed — show onboarding
        setIsOnboarded(false);
      });
  }, []);

  // Poll active window context every 3 seconds.
  // When WorkBuddy itself is focused, keep the previous context so the
  // detected-window badge doesn't disappear when the user clicks the input.
  useEffect(() => {
    const poll = setInterval(async () => {
      // Skip polling during streaming to reduce WebView2 IPC pressure
      if (isStreamingRef.current) return;
      try {
        const ctx = await invoke<WindowContext>("detect_active_window");
        // Refreshes on the next tick when the user switches back to
        // another app. Caveat: a stale badge can persist if that window
        // is closed without switching first — acceptable trade-off.
        const isSelf = ctx.title.toLowerCase().includes("workbuddy") ||
          ctx.title.toLowerCase().includes("workbuddy");
        if (!isSelf) {
          setCurrentContext(ctx);
        }
      } catch {
        // Window detection unavailable on this platform — that's okay
      }
    }, 3000);
    return () => clearInterval(poll);
  }, []);

  // Resize window based on expanded state (skip during onboarding — it manages its own height)
  useEffect(() => {
    if (!isOnboarded) return;
    const height = isExpanded ? 600 : 54;
    invoke("set_window_height", { height }).catch(() => {});
  }, [isExpanded, isOnboarded]);

  const updateSettings = useCallback((partial: Partial<Settings>) => {
    setSettings((prev) => {
      const updated = { ...prev, ...partial };
      // Merge api_keys rather than replace
      if (partial.api_keys) {
        updated.api_keys = { ...prev.api_keys, ...partial.api_keys };
      }
      invoke("set_settings", { settings: updated }).catch(() => {});
      return updated;
    });
  }, []);

  const addMessage = useCallback((msg: Message) => {
    setMessages((prev) => [...prev, msg]);
  }, []);

  const clearMessages = useCallback(() => {
    setMessages([]);
    conversationIdRef.current = null;
  }, []);

  return (
    <AppContext.Provider
      value={{
        settings,
        updateSettings,
        isExpanded,
        setIsExpanded,
        currentContext,
        messages,
        setMessages,
        addMessage,
        clearMessages,
        isStreaming,
        setIsStreaming,
        isOnboarded,
        setIsOnboarded,
        currentPage,
        setCurrentPage,
        screenshotDims,
        setScreenshotDims,
        conversationIdRef,
        externalQuestion,
        setExternalQuestion,
        journalChatContext,
        setJournalChatContext,
        submitExternalRef,
      }}
    >
      {children}
    </AppContext.Provider>
  );
}

export function useApp() {
  const ctx = useContext(AppContext);
  if (!ctx) throw new Error("useApp must be used within AppProvider");
  return ctx;
}
