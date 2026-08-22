import React, { createContext, useContext, useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Settings {
  api_keys: Record<string, string>;
  provider: string;
  model: string;
  theme: string;
  tutor_mode: boolean;
  a11y_detection_enabled: boolean;
  mask_form_inputs: boolean;
  extension_highlight_enabled: boolean;
  personal_dictionary: string[];
  browser_checking_enabled: boolean;
  excluded_hosts: string[];
  native_monitoring_enabled: boolean;
  excluded_processes: string[];
  widget_enabled: boolean;
  selection_hotkey_enabled: boolean;
  writing_goals: {
    dialect: string;
    domain: string;
    formality: string;
    audience: string;
    intent: string | null;
  };
  style_rules: { find: string; replace: string; case_sensitive: boolean }[];
  retain_snippets: boolean;
  snippets_enabled: boolean;
  snippets: { trigger: string; body: string; cursor_offset: number }[];
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
  // /ask from before the user closed and reopened WordBuddy).
  receivedAt: number;
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
  currentPage: "chat" | "settings" | "history" | "stats" | "onboarding";
  setCurrentPage: (p: "chat" | "settings" | "history" | "stats" | "onboarding") => void;
  screenshotDims: ScreenshotDimensions | null;
  setScreenshotDims: (d: ScreenshotDimensions | null) => void;
  conversationIdRef: React.MutableRefObject<string | null>;
  externalQuestion: ExternalQuestionPending | null;
  setExternalQuestion: (v: ExternalQuestionPending | null) => void;
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
  theme: "dark",
  tutor_mode: false,
  a11y_detection_enabled: true,
  mask_form_inputs: false,
  extension_highlight_enabled: true,
  personal_dictionary: [],
  browser_checking_enabled: true,
  excluded_hosts: [],
  native_monitoring_enabled: true,
  excluded_processes: [],
  widget_enabled: true,
  selection_hotkey_enabled: true,
  writing_goals: {
    dialect: "EnUs",
    domain: "General",
    formality: "Neutral",
    audience: "General",
    intent: null,
  },
  style_rules: [],
  retain_snippets: false,
  snippets_enabled: false,
  snippets: [],
};

const AppContext = createContext<AppState | null>(null);

export function AppProvider({ children }: { children: React.ReactNode }) {
  const [settings, setSettings] = useState<Settings>(defaultSettings);
  const [isExpanded, setIsExpanded] = useState(false);
  const [currentContext, setCurrentContext] = useState<WindowContext | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [isStreaming, setIsStreaming] = useState(false);
  const [isOnboarded, setIsOnboarded] = useState(false);
  const [currentPage, setCurrentPage] = useState<"chat" | "settings" | "history" | "stats" | "onboarding">("chat");
  const [screenshotDims, setScreenshotDims] = useState<ScreenshotDimensions | null>(null);
  const [externalQuestion, setExternalQuestion] = useState<ExternalQuestionPending | null>(null);
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
  // When WordBuddy itself is focused, keep the previous context so the
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
        const isSelf = ctx.title.toLowerCase().includes("wordbuddy");
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
