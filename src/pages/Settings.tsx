import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import {
  ArrowLeft,
  BookOpen,
  ScrollText,
  Key,
  Monitor,
  MousePointer2,
  Volume2,
  Mic,
  Loader2,
  CheckCircle,
  XCircle,
  Cpu,
  Info,
  Database,
  Globe,
  Accessibility,
  GraduationCap,
  Trash2,
  Eye,
  EyeOff,
  FolderOpen,
  Copy,
  Bug,
  Video,
} from "lucide-react";
import { KeyInput, cleanKey } from "../components/KeyInput";
import { open } from "@tauri-apps/plugin-shell";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useApp } from "../contexts/app.context";
interface ProviderModel {
  id: string;
  name: string;
}

interface ProviderInfo {
  id: string;
  name: string;
  key_required: boolean;
  models: ProviderModel[];
}

function Toggle({
  checked,
  onChange,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: () => void;
  disabled?: boolean;
  label: string;
}) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={onChange}
      disabled={disabled}
      className={`w-10 h-5 rounded-full transition-colors disabled:opacity-30 ${
        checked ? "bg-accent" : "bg-zinc-700"
      }`}
    >
      <div
        className={`w-4 h-4 bg-white rounded-full transition-transform mx-0.5 ${
          checked ? "translate-x-5" : ""
        }`}
      />
    </button>
  );
}

interface MonitorInfo {
  index: number;
  name: string;
  width: number;
  height: number;
  primary: boolean;
}

function MonitorSelector({
  value,
  onChange,
  autoValue = "auto",
  autoLabel = "Auto (primary)",
  allowAll = false,
}: {
  value: string;
  onChange: (v: string) => void;
  /** Value written by the first chip. The journal selector uses "" =
      "follow the screenshot monitor" instead of "auto". */
  autoValue?: string;
  autoLabel?: string;
  /** Adds an "All monitors" chip (journal recorder only — assistant
      screenshots need a single monitor for pointing coordinates). */
  allowAll?: boolean;
}) {
  const [monitors, setMonitors] = useState<MonitorInfo[]>([]);

  useEffect(() => {
    invoke<MonitorInfo[]>("list_monitors").then(setMonitors).catch(() => {});
  }, []);

  const chip = (selected: boolean) =>
    `py-1.5 px-3 text-xs rounded-lg border transition-colors ${
      selected
        ? "border-accent bg-accent/10 text-white"
        : "border-zinc-700 bg-zinc-900 text-zinc-400 hover:border-zinc-600"
    }`;

  return (
    <div className="flex gap-2 flex-wrap">
      <button onClick={() => onChange(autoValue)} className={chip(value === autoValue)}>
        {autoLabel}
      </button>
      {allowAll && (
        <button onClick={() => onChange("all")} className={chip(value === "all")}>
          All monitors
        </button>
      )}
      {monitors.map((m) => (
        <button
          key={m.index}
          onClick={() => onChange(String(m.index))}
          className={`py-1.5 px-3 text-xs rounded-lg border transition-colors ${
            value === String(m.index)
              ? "border-accent bg-accent/10 text-white"
              : "border-zinc-700 bg-zinc-900 text-zinc-400 hover:border-zinc-600"
          }`}
        >
          {m.primary ? "Primary" : `Monitor ${m.index + 1}`} ({m.width}x{m.height})
        </button>
      ))}
    </div>
  );
}

interface TtsVoice {
  id: string;
  name: string;
}

/// Controlled OpenAI key input used when the user wants Whisper STT but
/// their main LLM provider isn't OpenAI (so no `openai` key is set).
/// Saves on blur; uses controlled state so closing Settings mid-typing
/// doesn't silently lose the value.
///
/// The props-driven resync on `initial` change assumes INV-ARCH-014 (one
/// Settings panel mounted at a time). If a multi-window refactor ever
/// mounts two Settings instances simultaneously, an external save from
/// one would overwrite an in-progress typed value in the other via the
/// `useEffect(…, [initial])` below — revisit this then.
function SttKeyInput({
  initial,
  updateSettings,
}: {
  initial: string;
  updateSettings: (s: Record<string, unknown>) => void;
}) {
  const [val, setVal] = useState(initial);
  // Keep the field in sync if settings change elsewhere (e.g., the user
  // saves a key via the main provider input and we share `stt` alias).
  // Relies on INV-ARCH-014 — see the doc comment above the component.
  useEffect(() => {
    setVal(initial);
  }, [initial]);
  const handleSave = (cleaned: string) => {
    if (!cleaned) return;
    invoke("set_api_key", { service: "stt", key: cleaned }).catch(() => {});
    updateSettings({ api_keys: { stt: cleaned } });
    setVal(cleaned);
  };
  return (
    <div className="space-y-2">
      <label htmlFor="stt-key" className="text-xs text-zinc-500">
        OpenAI API Key (for Whisper STT)
      </label>
      <KeyInput
        id="stt-key"
        value={val}
        onSave={handleSave}
        placeholder="sk-..."
        ariaLabel="OpenAI API Key for Whisper STT"
      />
    </div>
  );
}

interface RecorderStatus {
  running: boolean;
  interval_secs: number;
  retention_days: number;
  shots_taken: number;
  shots_skipped_idle: number;
  last_capture_at: number;
  last_error: string | null;
  recordings_dir: string;
}

/// Work-journal recorder controls (ADR-042). The toggle both persists the
/// setting (so the recorder resumes on app restart) and starts/stops the
/// capture loop immediately.
function RecorderSection({
  settings,
  updateSettings,
}: {
  settings: {
    recorder_enabled: boolean;
    recorder_interval_secs: number;
    recorder_retention_days: number;
    journal_capture_monitor: string;
    analysis_provider: string;
    analysis_model: string;
  };
  updateSettings: (s: Record<string, unknown>) => void;
}) {
  const [status, setStatus] = useState<RecorderStatus | null>(null);

  useEffect(() => {
    let cancelled = false;
    const fetchStatus = () => {
      invoke<RecorderStatus>("recorder_status")
        .then((s) => { if (!cancelled) setStatus(s); })
        .catch(() => {});
    };
    fetchStatus();
    const interval = setInterval(fetchStatus, 5000);
    return () => { cancelled = true; clearInterval(interval); };
  }, []);

  const handleToggle = async () => {
    const next = !settings.recorder_enabled;
    updateSettings({ recorder_enabled: next });
    try {
      const s = await invoke<RecorderStatus>(next ? "recorder_start" : "recorder_stop");
      setStatus(s);
    } catch (err) {
      console.warn("[recorder] toggle failed:", err);
    }
  };

  return (
    <section id="section-recorder" className="space-y-3 pt-2 border-t border-zinc-800/50">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
            <Video size={14} /> Work Journal Recorder
          </h2>
          <p className="text-xs text-zinc-600">
            Captures a screenshot every few seconds so WorkBuddy can build an
            automatic timeline of your day. Frames stay on this machine,
            are skipped while you're idle, and are deleted after the
            retention period. Off by default.
          </p>
        </div>
        <Toggle
          checked={settings.recorder_enabled}
          onChange={handleToggle}
          label="Work journal recorder"
        />
      </div>

      <div className="flex items-center gap-2">
        <div
          className={`w-2 h-2 rounded-full ${
            status?.running ? "bg-red-500 animate-pulse" : "bg-zinc-600"
          }`}
        />
        <span className={`text-xs ${status?.running ? "text-red-400" : "text-zinc-500"}`}>
          {status?.running
            ? `Recording — ${status.shots_taken} frames this session${
                status.shots_skipped_idle > 0
                  ? `, ${status.shots_skipped_idle} skipped (idle)`
                  : ""
              }`
            : "Not recording"}
        </span>
      </div>
      {status?.last_error && (
        <p className="text-xs text-red-400">Last error: {status.last_error}</p>
      )}

      <div className="grid grid-cols-2 gap-3">
        <div className="space-y-1">
          <label htmlFor="recorder-interval" className="text-xs text-zinc-500">
            Capture interval (seconds)
          </label>
          <input
            id="recorder-interval"
            type="number"
            min={2}
            max={600}
            value={settings.recorder_interval_secs}
            onChange={(e) => {
              const v = parseInt(e.target.value, 10);
              if (Number.isFinite(v)) {
                updateSettings({ recorder_interval_secs: Math.min(600, Math.max(2, v)) });
              }
            }}
            className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-accent/50"
          />
        </div>
        <div className="space-y-1">
          <label htmlFor="recorder-retention" className="text-xs text-zinc-500">
            Keep frames for (days)
          </label>
          <input
            id="recorder-retention"
            type="number"
            min={1}
            max={365}
            value={settings.recorder_retention_days}
            onChange={(e) => {
              const v = parseInt(e.target.value, 10);
              if (Number.isFinite(v)) {
                updateSettings({ recorder_retention_days: Math.min(365, Math.max(1, v)) });
              }
            }}
            className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-accent/50"
          />
        </div>
      </div>
      <div className="space-y-1">
        <span className="text-xs text-zinc-500">Journal captures</span>
        <MonitorSelector
          value={settings.journal_capture_monitor || ""}
          onChange={(v) => updateSettings({ journal_capture_monitor: v })}
          autoValue=""
          autoLabel="Same as screenshots"
          allowAll
        />
        <p className="text-[11px] text-zinc-600">
          "All monitors" stitches every screen side-by-side into one frame,
          so the journal sees your whole desk. Wider frames cost roughly one
          extra screen's worth of analysis tokens per capture.
        </p>
      </div>

      {status?.recordings_dir && (
        <p className="text-xs text-zinc-600">
          Frames: <code className="text-zinc-400">{status.recordings_dir}</code>.
          Timeline summaries are kept even after frames expire.
        </p>
      )}

      {/* Analysis provider/model — which LLM turns frames into the
          timeline. Empty = follow the chat provider/model, so most users
          never touch this; power users can point analysis at a cheaper
          or local model. */}
      <div className="grid grid-cols-2 gap-3 pt-2 border-t border-zinc-800/50">
        <div className="space-y-1">
          <label htmlFor="analysis-provider" className="text-xs text-zinc-500">
            Analysis provider
          </label>
          <select
            id="analysis-provider"
            value={settings.analysis_provider || ""}
            onChange={(e) => updateSettings({ analysis_provider: e.target.value })}
            className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-accent/50"
          >
            <option value="">Same as chat</option>
            <option value="anthropic">Anthropic</option>
            <option value="openai">OpenAI</option>
            <option value="google">Google</option>
            <option value="groq">Groq</option>
            <option value="ollama">Ollama</option>
            <option value="openrouter">OpenRouter</option>
          </select>
        </div>
        <div className="space-y-1">
          <label htmlFor="analysis-model" className="text-xs text-zinc-500">
            Analysis model
          </label>
          <input
            id="analysis-model"
            type="text"
            value={settings.analysis_model || ""}
            onChange={(e) => updateSettings({ analysis_model: e.target.value })}
            placeholder="Same as chat"
            className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-accent/50"
          />
        </div>
      </div>
    </section>
  );
}

/// Wotch launch settings. Described in docs/WOTCH_INTEGRATION.md.
function ClaudeCodeIntegrationSection({
  settings,
  updateSettings,
}: {
  settings: {
    wotch_integration_enabled: boolean;
  };
  updateSettings: (s: Record<string, unknown>) => void;
}) {
  const [wotchStatus, setWotchStatus] = useState<{
    installed: boolean;
    path: string | null;
    running: boolean;
    port: number | null;
  } | null>(null);

  useEffect(() => {
    invoke<typeof wotchStatus>("wotch_status")
      .then(setWotchStatus)
      .catch(() => setWotchStatus(null));
  }, []);

  return (
    <section className="space-y-3 pt-2 border-t border-zinc-800/50">
      <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
        <Cpu size={14} /> Wotch
      </h2>
      <p className="text-xs text-zinc-600">
        Launch Wotch (floating terminal) from the chat toolbar.
        See docs/WOTCH_INTEGRATION.md.
      </p>

      {/* Wotch integration toggle */}
      <div className="flex items-start justify-between gap-3">
        <div>
          <h3 className="text-xs font-semibold text-zinc-300">
            Show "Open in Wotch" button
          </h3>
          <p className="text-xs text-zinc-600">
            Adds a terminal-icon button to the chat toolbar that launches{" "}
            <a
              href="#"
              onClick={(e) => {
                e.preventDefault();
                open("https://github.com/Frostbite1536/Wotch").catch(() => {});
              }}
              className="text-accent hover:underline cursor-pointer"
            >
              Wotch
            </a>
            {" "}(if installed) with the current input pre-filled as a Claude
            Code prompt. Button is hidden when Wotch isn't detected.
          </p>
          {wotchStatus && (
            <p className={`text-xs mt-1 ${wotchStatus.installed ? "text-accent" : "text-zinc-500"}`}>
              {wotchStatus.installed
                ? `Detected at: ${wotchStatus.path}${wotchStatus.running ? ` (running on :${wotchStatus.port})` : ""}`
                : "Wotch not detected on this machine."}
            </p>
          )}
        </div>
        <Toggle
          checked={settings.wotch_integration_enabled}
          onChange={() =>
            updateSettings({
              wotch_integration_enabled: !settings.wotch_integration_enabled,
            })
          }
          label="Wotch integration"
        />
      </div>
    </section>
  );
}

/// Inline Google API key input. Used from TTS and STT sections when the
/// user selects Gemini but hasn't configured the `google` key via the
/// main AI Provider section (because their LLM provider is different).
function GoogleKeyInput({
  updateSettings,
}: {
  updateSettings: (s: Record<string, unknown>) => void;
}) {
  const [key, setKey] = useState("");
  const [saved, setSaved] = useState(false);
  const handleSave = async (cleaned: string) => {
    if (!cleaned) return;
    try {
      await invoke("set_api_key", { service: "google", key: cleaned });
      updateSettings({ api_keys: { google: cleaned } });
      setKey(cleaned);
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch {
      // Save failed
    }
  };
  return (
    <div className="space-y-1">
      <label htmlFor="google-key-inline" className="text-xs text-zinc-500">
        Google API Key (for Gemini){" "}
        <a
          href="#"
          onClick={(e) => {
            e.preventDefault();
            open("https://aistudio.google.com/apikey").catch(() => {});
          }}
          className="text-accent hover:underline cursor-pointer"
        >
          Get a key
        </a>
      </label>
      <div className="flex gap-2">
        <KeyInput
          id="google-key-inline"
          value={key}
          onSave={handleSave}
          placeholder="AIza..."
          ariaLabel="Google API Key for Gemini"
          className="flex-1"
        />
        {saved && (
          <span className="px-3 py-2 text-accent" aria-label="Saved">
            <CheckCircle size={16} />
          </span>
        )}
      </div>
    </div>
  );
}

function TTSSection({
  settings,
  updateSettings,
}: {
  settings: {
    tts_enabled: boolean;
    tts_provider: string;
    tts_voice: string;
    api_keys?: Record<string, string>;
  };
  updateSettings: (s: Record<string, unknown>) => void;
}) {
  const [voices, setVoices] = useState<TtsVoice[]>([]);

  useEffect(() => {
    // Cancelled-flag pattern: a rapid provider toggle can otherwise race
    // and overwrite the newer voice list with the older resolve.
    let cancelled = false;
    invoke<TtsVoice[]>("list_tts_voices", {
      provider: settings.tts_provider || "elevenlabs",
    })
      .then((v) => { if (!cancelled) setVoices(v); })
      .catch(() => { if (!cancelled) setVoices([]); });
    return () => { cancelled = true; };
  }, [settings.tts_provider]);

  const hasElevenKey = !!settings.api_keys?.elevenlabs;
  const hasGoogleKey = !!settings.api_keys?.google;
  const hasKey =
    settings.tts_provider === "gemini" ? hasGoogleKey : hasElevenKey;

  const hint = hasKey
    ? settings.tts_provider === "gemini"
      ? "Reads responses aloud via Gemini (uses your Google key)"
      : "Reads responses aloud via ElevenLabs"
    : settings.tts_provider === "gemini"
      ? "Add a Google API key below to enable"
      : "Add an ElevenLabs API key above to enable";

  return (
    <section className="space-y-3">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
            <Volume2 size={14} /> Voice Responses
          </h2>
          <p className="text-xs text-zinc-600">{hint}</p>
        </div>
        <Toggle
          checked={settings.tts_enabled}
          onChange={() =>
            updateSettings({ tts_enabled: !settings.tts_enabled })
          }
          disabled={!hasKey}
          label="Voice responses"
        />
      </div>

      {/* Provider selector */}
      <div className="space-y-1">
        <label className="text-xs text-zinc-500">TTS Provider</label>
        <div className="flex gap-2">
          {[
            {
              id: "elevenlabs",
              name: "ElevenLabs",
              hint: "Premium quality, separate API key",
            },
            {
              id: "gemini",
              name: "Gemini Flash",
              hint: "Reuses Google API key, 30 voices, lower cost",
            },
          ].map((p) => (
            <button
              key={p.id}
              onClick={() =>
                updateSettings({ tts_provider: p.id, tts_voice: "default" })
              }
              title={p.hint}
              className={`flex-1 py-1.5 text-xs rounded-lg border transition-colors ${
                settings.tts_provider === p.id
                  ? "border-accent bg-accent/10 text-white"
                  : "border-zinc-700 bg-zinc-900 text-zinc-400 hover:border-zinc-600"
              }`}
            >
              {p.name}
            </button>
          ))}
        </div>
      </div>

      {/* Voice selector */}
      {voices.length > 0 && (
        <div className="space-y-1">
          <label htmlFor="tts-voice" className="text-xs text-zinc-500">
            Voice
          </label>
          <select
            id="tts-voice"
            value={settings.tts_voice || "default"}
            onChange={(e) => updateSettings({ tts_voice: e.target.value })}
            className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-accent/50"
          >
            {voices.map((v) => (
              <option key={v.id} value={v.id}>
                {v.name}
              </option>
            ))}
          </select>
        </div>
      )}

      {/* Inline Google key input when Gemini is selected without a key.
          Mirrors the existing ElevenLabs-key UX so users don't have to
          temporarily switch their LLM provider to Google just to enter
          a key for TTS. */}
      {settings.tts_provider === "gemini" && !hasGoogleKey && (
        <GoogleKeyInput updateSettings={updateSettings} />
      )}
    </section>
  );
}

function ExtensionSection({
  settings,
  updateSettings,
}: {
  settings: { mask_form_inputs: boolean; extension_highlight_enabled: boolean };
  updateSettings: (s: Record<string, unknown>) => void;
}) {
  const [status, setStatus] = useState<{
    connected: boolean;
    port: number;
    token: string;
    element_count: number;
    page_url: string;
    page_title: string;
  } | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    invoke<typeof status>("get_extension_status").then(setStatus).catch(() => {});
    const interval = setInterval(() => {
      invoke<typeof status>("get_extension_status").then(setStatus).catch(() => {});
    }, 5000);
    return () => clearInterval(interval);
  }, []);

  const handleCopyToken = () => {
    if (status?.token) {
      navigator.clipboard.writeText(status.token);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  const handleRegenerate = async () => {
    try {
      const newToken = await invoke<string>("regenerate_extension_token");
      if (status) setStatus({ ...status, token: newToken });
    } catch { /* ignore */ }
  };

  return (
    <section className="space-y-3">
      <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
        <Globe size={14} /> Browser Extension
      </h2>
      <p className="text-xs text-zinc-600">
        Instant element detection for web-based content. Replaces YOLO+OCR on browser pages.
      </p>

      <div className="flex items-center gap-2">
        <div className={`w-2 h-2 rounded-full ${status?.connected ? "bg-emerald-500" : "bg-zinc-600"}`} />
        <span className={`text-xs ${status?.connected ? "text-emerald-400" : "text-zinc-500"}`}>
          {status?.connected
            ? `Connected \u2014 ${status.element_count} elements on ${status.page_title || "page"}`
            : "Not connected"}
        </span>
      </div>

      {status && (
        <div className="space-y-2">
          <div>
            <label className="text-xs text-zinc-500">Port</label>
            <p className="text-xs text-zinc-300">{status.port}</p>
          </div>
          <div>
            <label className="text-xs text-zinc-500">Auth Token</label>
            <div className="flex gap-2 items-center">
              <code className="text-xs text-zinc-400 bg-zinc-900 px-2 py-1 rounded flex-1 overflow-hidden text-ellipsis whitespace-nowrap">
                {status.token.slice(0, 20)}...
              </code>
              <button
                onClick={handleCopyToken}
                className="text-xs text-zinc-400 hover:text-zinc-200 transition-colors"
              >
                {copied ? "Copied!" : "Copy"}
              </button>
              <button
                onClick={handleRegenerate}
                className="text-xs text-zinc-500 hover:text-zinc-300 transition-colors"
              >
                Regenerate
              </button>
            </div>
          </div>
          {!status.connected && (
            <p className="text-xs text-zinc-600">
              Load the extension from{" "}
              <code className="text-zinc-400">workbuddy-extension/</code> via
              chrome://extensions (enable Developer mode). Paste the token above
              into the extension popup.
            </p>
          )}
        </div>
      )}

      {/* Privacy: mask form-input values */}
      <div className="flex items-start justify-between gap-3 pt-2 border-t border-zinc-800/50">
        <div>
          <h3 className="text-xs font-semibold text-zinc-300">
            Mask form inputs
          </h3>
          <p className="text-xs text-zinc-600">
            Replace user-entered values in {"<input>"} and {"<textarea>"} fields
            with a type-aware placeholder (e.g. <code className="text-zinc-400">[input: email]</code>)
            before sending to the LLM. Password fields are always masked
            regardless. Field position and type are preserved for context.
          </p>
        </div>
        <Toggle
          checked={settings.mask_form_inputs}
          onChange={() =>
            updateSettings({ mask_form_inputs: !settings.mask_form_inputs })
          }
          label="Mask form inputs"
        />
      </div>

      {/* Highlight routing: extension vs cursor overlay */}
      <div className="flex items-start justify-between gap-3 pt-2 border-t border-zinc-800/50">
        <div>
          <h3 className="text-xs font-semibold text-zinc-300">
            Use extension for highlights
          </h3>
          <p className="text-xs text-zinc-600">
            When on, the <strong>highlight</strong> tool paints an in-page
            rectangle via the extension on browser pages (scrolls with the
            page, no second overlay window). Falls back to the cursor overlay
            when the extension isn't connected. Precise pointing
            (<strong>point_at</strong>) always uses the cursor overlay.
          </p>
        </div>
        <Toggle
          checked={settings.extension_highlight_enabled}
          onChange={() =>
            updateSettings({
              extension_highlight_enabled: !settings.extension_highlight_enabled,
            })
          }
          label="Route highlights through extension"
        />
      </div>
    </section>
  );
}

const PROVIDER_KEY_NAMES: Record<string, string> = {
  anthropic: "Anthropic",
  openai: "OpenAI",
  google: "Google AI",
  groq: "Groq",
  openrouter: "OpenRouter",
};

const PROVIDER_KEY_PLACEHOLDERS: Record<string, string> = {
  anthropic: "sk-ant-...",
  openai: "sk-...",
  google: "AIza...",
  groq: "gsk_...",
  openrouter: "sk-or-...",
};

const SETTINGS_HEIGHT = 600;

export default function Settings() {
  const { settings, updateSettings, setCurrentPage, isExpanded } = useApp();

  // Expand window for settings, collapse when navigating back
  useEffect(() => {
    invoke("set_window_height", { height: SETTINGS_HEIGHT }).catch(() => {});
  }, []);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [providerKey, setProviderKey] = useState(
    settings.api_keys?.[settings.provider] || "",
  );
  const [showProviderKey, setShowProviderKey] = useState(false);
  const [showElevenLabsKey, setShowElevenLabsKey] = useState(false);
  const [elevenLabsKey, setElevenLabsKey] = useState(
    settings.api_keys?.elevenlabs || "",
  );
  const [validating, setValidating] = useState(false);
  const [validationResult, setValidationResult] = useState<boolean | null>(null);
  const [elevenLabsSaved, setElevenLabsSaved] = useState(false);
  const [appVersion, setAppVersion] = useState("");
  const [ragStatus, setRagStatus] = useState<{
    total_chunks: number;
    source_files: string[];
    last_ingested: number | null;
  } | null>(null);
  const [indexing, setIndexing] = useState(false);
  const [indexResult, setIndexResult] = useState<string | null>(null);
  const [docsPath, setDocsPath] = useState("");

  // Load available providers, app version, and RAG status
  useEffect(() => {
    invoke<ProviderInfo[]>("list_providers")
      .then(setProviders)
      .catch(() => {});
    getVersion().then(setAppVersion).catch(() => {});
    invoke<{ total_chunks: number; source_files: string[]; last_ingested: number | null }>(
      "get_ingestion_status",
    )
      .then(setRagStatus)
      .catch(() => {});
  }, []);

  // Update key field when provider changes
  useEffect(() => {
    setProviderKey(settings.api_keys?.[settings.provider] || "");
    setValidationResult(null);
  }, [settings.provider, settings.api_keys]);

  const currentProvider = providers.find((p) => p.id === settings.provider);
  const currentModels = currentProvider?.models || [];

  const handleSaveProviderKey = async () => {
    setValidating(true);
    setValidationResult(null);
    // U11 audit: trim + strip embedded whitespace before validate
    // and persist. A pasted key with a trailing newline used to
    // fail validation cryptically.
    const cleaned = cleanKey(providerKey);
    try {
      // For Anthropic, validate via API call
      if (settings.provider === "anthropic") {
        const valid = await invoke<boolean>("validate_api_key", {
          key: cleaned,
        });
        setValidationResult(valid);
        if (!valid) {
          setValidating(false);
          return;
        }
      } else {
        // For other providers, just save (no validation endpoint)
        setValidationResult(true);
      }
      await invoke("set_api_key", {
        service: settings.provider,
        key: cleaned,
      });
      setProviderKey(cleaned);
      updateSettings({
        api_keys: { [settings.provider]: cleaned },
      });
    } catch {
      setValidationResult(false);
    }
    setValidating(false);
  };

  const handleSaveElevenLabsKey = async () => {
    const cleaned = cleanKey(elevenLabsKey);
    try {
      await invoke("set_api_key", {
        service: "elevenlabs",
        key: cleaned,
      });
      setElevenLabsKey(cleaned);
      // Only flip tts_enabled when the user is actually on the ElevenLabs
      // provider — otherwise saving an ElevenLabs key (e.g. to use Scribe
      // for STT) would unexpectedly re-enable TTS on Gemini. If tts_enabled
      // is already true (e.g. Gemini TTS was deliberately turned on), we
      // must not clobber that either.
      const patch: Record<string, unknown> = {
        api_keys: { elevenlabs: cleaned },
      };
      if (settings.tts_provider === "elevenlabs" && !settings.tts_enabled) {
        // Use the cleaned value, not the pre-clean state (the
        // setElevenLabsKey above hasn't flushed yet — React state
        // is batched). PR #33 P2 fix.
        patch.tts_enabled = !!cleaned;
      }
      updateSettings(patch);
      setElevenLabsSaved(true);
      setTimeout(() => setElevenLabsSaved(false), 2000);
    } catch {
      // Save failed
    }
  };

  const handleProviderChange = (newProvider: string) => {
    const provider = providers.find((p) => p.id === newProvider);
    const defaultModel = provider?.models[0]?.id || "";
    updateSettings({ provider: newProvider, model: defaultModel });
  };

  // U14 audit: jump-nav targets so the 13+ sections aren't a single
  // 580-line scroll. Anchored to section ids added below; clicking a
  // chip uses scrollIntoView + smooth scroll inside the page's
  // overflow-y-auto container.
  const sectionAnchors: Array<{ id: string; label: string }> = [
    { id: "section-ai", label: "AI" },
    { id: "section-keys", label: "Keys" },
    { id: "section-capture", label: "Capture" },
    { id: "section-recorder", label: "Journal" },
    { id: "section-voice", label: "Voice" },
    { id: "section-rag", label: "RAG" },
    { id: "section-integrations", label: "Wotch" },
    { id: "section-diagnostics", label: "Diagnostics" },
    { id: "section-about", label: "About" },
  ];
  const jumpTo = (id: string) => {
    const el = document.getElementById(id);
    if (el) el.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  return (
    <div className="h-full bg-background-primary text-zinc-100 overflow-y-auto">
      <div className="max-w-lg mx-auto p-6 space-y-6">
        {/* Header */}
        <div className="flex items-center gap-3">
          <button
            onClick={() => {
              invoke("set_window_height", { height: isExpanded ? 600 : 54 }).catch(() => {});
              setCurrentPage("chat");
            }}
            aria-label="Back to chat"
            className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-400"
          >
            <ArrowLeft size={18} />
          </button>
          <h1 className="text-lg font-heading font-semibold">Settings</h1>
        </div>

        {/* Section jump-nav (U14 audit). Sticks to the top of the
            scroll container so the user can hop between sections
            without a 580-line scroll hunt. */}
        <nav
          aria-label="Settings sections"
          className="sticky top-0 z-10 -mx-6 px-6 py-2 bg-background-primary/95 backdrop-blur border-b border-zinc-800/60 flex flex-wrap gap-1.5"
        >
          {sectionAnchors.map((s) => (
            <button
              key={s.id}
              type="button"
              onClick={() => jumpTo(s.id)}
              className="px-2 py-0.5 text-[11px] rounded-md bg-zinc-800/60 text-zinc-400 hover:bg-zinc-700 hover:text-zinc-200 transition-colors"
            >
              {s.label}
            </button>
          ))}
        </nav>

        {/* LLM Provider */}
        <section id="section-ai" className="space-y-3">
          <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
            <Cpu size={14} /> AI Provider
          </h2>
          <select
            value={settings.provider}
            onChange={(e) => handleProviderChange(e.target.value)}
            aria-label="AI provider"
            className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-accent/50"
          >
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
          {currentProvider && !currentProvider.key_required && (
            <p className="text-xs text-accent">
              No API key required — runs locally on your machine
            </p>
          )}
        </section>

        {/* API Keys */}
        <section id="section-keys" className="space-y-3">
          <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
            <Key size={14} /> API Keys
          </h2>

          {/* Provider API Key */}
          {currentProvider?.key_required && (
            <div className="space-y-2">
              <label htmlFor="provider-key" className="text-xs text-zinc-500">
                {PROVIDER_KEY_NAMES[settings.provider] || settings.provider} API Key
              </label>
              <div className="flex gap-2">
                <div className="relative flex-1">
                  <input
                    id="provider-key"
                    type={showProviderKey ? "text" : "password"}
                    value={providerKey}
                    onChange={(e) => {
                      setProviderKey(e.target.value);
                      setValidationResult(null);
                    }}
                    placeholder={
                      PROVIDER_KEY_PLACEHOLDERS[settings.provider] || "API key..."
                    }
                    className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 pr-9 text-sm font-mono focus:outline-none focus:border-accent/50"
                  />
                  <button
                    type="button"
                    onClick={() => setShowProviderKey((s) => !s)}
                    aria-label={showProviderKey ? "Hide key" : "Show key"}
                    title={showProviderKey ? "Hide" : "Show"}
                    className="absolute right-2 top-1/2 -translate-y-1/2 p-1 text-zinc-500 hover:text-zinc-300"
                  >
                    {showProviderKey ? <EyeOff size={14} /> : <Eye size={14} />}
                  </button>
                </div>
                <button
                  onClick={handleSaveProviderKey}
                  disabled={!providerKey || validating}
                  className="px-3 py-2 bg-accent/20 text-accent rounded-lg text-sm hover:bg-accent/30 disabled:opacity-30"
                >
                  {validating ? (
                    <Loader2 size={16} className="animate-spin" />
                  ) : validationResult === true ? (
                    <CheckCircle size={16} />
                  ) : validationResult === false ? (
                    <XCircle size={16} />
                  ) : (
                    "Save"
                  )}
                </button>
              </div>
            </div>
          )}

          {/* Ollama URL */}
          {settings.provider === "ollama" && (
            <div className="space-y-2">
              <label htmlFor="ollama-url" className="text-xs text-zinc-500">
                Ollama URL (default: http://localhost:11434)
              </label>
              <div className="flex gap-2">
                <input
                  id="ollama-url"
                  type="text"
                  value={settings.api_keys?.ollama_url || ""}
                  onChange={(e) => {
                    invoke("set_api_key", {
                      service: "ollama_url",
                      key: e.target.value,
                    }).catch(() => {});
                    updateSettings({
                      api_keys: { ollama_url: e.target.value },
                    });
                  }}
                  placeholder="http://localhost:11434"
                  className="flex-1 bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-accent/50"
                />
              </div>
            </div>
          )}

          {/* ElevenLabs Key */}
          <div className="space-y-2">
            <label htmlFor="elevenlabs-key" className="text-xs text-zinc-500">
              ElevenLabs API Key (optional — enables voice responses){" "}
              <a
                href="#"
                onClick={(e) => {
                  e.preventDefault();
                  open("https://elevenlabs.io/app/developers/api-keys").catch(() => {});
                }}
                className="text-accent hover:underline cursor-pointer"
              >
                Get a key
              </a>
            </label>
            <div className="flex gap-2">
              <div className="relative flex-1">
                <input
                  id="elevenlabs-key"
                  type={showElevenLabsKey ? "text" : "password"}
                  value={elevenLabsKey}
                  onChange={(e) => setElevenLabsKey(e.target.value)}
                  placeholder="xi-..."
                  className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 pr-9 text-sm font-mono focus:outline-none focus:border-accent/50"
                />
                <button
                  type="button"
                  onClick={() => setShowElevenLabsKey((s) => !s)}
                  aria-label={showElevenLabsKey ? "Hide key" : "Show key"}
                  title={showElevenLabsKey ? "Hide" : "Show"}
                  className="absolute right-2 top-1/2 -translate-y-1/2 p-1 text-zinc-500 hover:text-zinc-300"
                >
                  {showElevenLabsKey ? <EyeOff size={14} /> : <Eye size={14} />}
                </button>
              </div>
              <button
                onClick={handleSaveElevenLabsKey}
                disabled={!elevenLabsKey}
                className="px-3 py-2 bg-zinc-800 text-zinc-300 rounded-lg text-sm hover:bg-zinc-700 disabled:opacity-30"
              >
                {elevenLabsSaved ? <CheckCircle size={16} /> : "Save"}
              </button>
            </div>
          </div>
        </section>

        {/* Model */}
        <section className="space-y-3">
          <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
            <Monitor size={14} /> Model
          </h2>
          <select
            value={settings.model}
            onChange={(e) => updateSettings({ model: e.target.value })}
            aria-label="AI model"
            className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-accent/50"
          >
            {currentModels.map((m) => (
              <option key={m.id} value={m.id}>
                {m.name}
              </option>
            ))}
          </select>
        </section>

        {/* Tutor Mode */}
        <section className="space-y-3">
          <div className="flex items-center justify-between">
            <div>
              <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
                <BookOpen size={14} /> Tutor Mode
              </h2>
              <p className="text-xs text-zinc-600">
                Socratic teaching — asks questions, guides interactions, one concept at a time
              </p>
            </div>
            <Toggle
              checked={settings.tutor_mode}
              onChange={() =>
                updateSettings({ tutor_mode: !settings.tutor_mode })
              }
              label="Tutor mode"
            />
          </div>
        </section>

        {/* Auto-screenshot */}
        <section id="section-capture" className="space-y-3">
          <div className="flex items-center justify-between">
            <div>
              <h2 className="text-sm font-semibold text-zinc-400">Auto-screenshot</h2>
              <p className="text-xs text-zinc-600">
                Capture screen with every question for context
              </p>
            </div>
            <Toggle
              checked={settings.auto_screenshot}
              onChange={() =>
                updateSettings({ auto_screenshot: !settings.auto_screenshot })
              }
              label="Auto-screenshot"
            />
          </div>
        </section>

        {/* Cursor Overlay */}
        <section className="space-y-3">
          <div className="flex items-center justify-between">
            <div>
              <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
                <MousePointer2 size={14} /> Cursor Overlay
              </h2>
              <p className="text-xs text-zinc-600">
                Animated cursor points at UI elements on your screen.
                Uses a full-screen transparent overlay window.
              </p>
            </div>
            <Toggle
              checked={settings.cursor_overlay_enabled}
              onChange={() =>
                updateSettings({ cursor_overlay_enabled: !settings.cursor_overlay_enabled })
              }
              label="Cursor overlay"
            />
          </div>
        </section>

        {/* Capture Monitor */}
        <section className="space-y-3">
          <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
            <Monitor size={14} /> Capture Monitor
          </h2>
          <p className="text-xs text-zinc-600">
            Which monitor to screenshot. Use "Auto" or select a specific monitor.
          </p>
          <MonitorSelector
            value={settings.capture_monitor || "auto"}
            onChange={(val) => {
              updateSettings({ capture_monitor: val });
            }}
          />
        </section>

        {/* Work Journal Recorder (ADR-042) */}
        <RecorderSection settings={settings} updateSettings={updateSettings} />

        {/* Browser Extension */}
        <ExtensionSection settings={settings} updateSettings={updateSettings} />

        {/* Accessibility-powered UI Detection */}
        <section className="space-y-3">
          <div className="flex items-center justify-between">
            <div>
              <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
                <Accessibility size={14} /> Accessibility Detection
              </h2>
              <p className="text-xs text-zinc-600">
                Read UI element names + positions from the foreground window's
                accessibility tree for pixel-precise pointing. Works best in
                IDEs, terminals, and Electron apps. Data stays local.
              </p>
            </div>
            <Toggle
              checked={settings.a11y_detection_enabled}
              onChange={() =>
                updateSettings({
                  a11y_detection_enabled: !settings.a11y_detection_enabled,
                })
              }
              label="Accessibility detection"
            />
          </div>
        </section>

        {/* TTS */}
        <section id="section-voice" className="space-y-3">
          <TTSSection settings={settings} updateSettings={updateSettings} />
        </section>

        {/* Speech-to-Text */}
        <section className="space-y-3">
          <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
            <Mic size={14} /> Speech-to-Text
          </h2>
          <p className="text-xs text-zinc-600">
            Push-to-talk transcription provider. ElevenLabs reuses your TTS key
            — no extra API key needed.
          </p>
          <div className="space-y-2">
            <label className="text-xs text-zinc-500">STT Provider</label>
            <div className="flex gap-2">
              {[
                { id: "whisper", name: "OpenAI Whisper" },
                { id: "elevenlabs", name: "ElevenLabs" },
                { id: "gemini", name: "Gemini Flash" },
              ].map((p) => (
                <button
                  key={p.id}
                  onClick={() => updateSettings({ stt_provider: p.id })}
                  className={`flex-1 py-1.5 text-xs rounded-lg border transition-colors ${
                    settings.stt_provider === p.id
                      ? "border-accent bg-accent/10 text-white"
                      : "border-zinc-700 bg-zinc-900 text-zinc-400 hover:border-zinc-600"
                  }`}
                >
                  {p.name}
                </button>
              ))}
            </div>
          </div>
          {settings.stt_provider === "whisper" &&
            settings.provider !== "openai" &&
            !settings.api_keys?.openai && (
              <SttKeyInput
                initial={settings.api_keys?.stt || ""}
                updateSettings={updateSettings}
              />
            )}
          {settings.stt_provider === "elevenlabs" && !settings.api_keys?.elevenlabs && (
            <p className="text-xs text-zinc-500">
              Add an ElevenLabs API key above to enable. Requires{" "}
              <strong>Speech to Text</strong> permission on your key.
            </p>
          )}
          {settings.stt_provider === "gemini" && !settings.api_keys?.google && (
            <GoogleKeyInput updateSettings={updateSettings} />
          )}
          {((settings.stt_provider === "whisper" &&
            (settings.api_keys?.openai || settings.api_keys?.stt)) ||
            (settings.stt_provider === "elevenlabs" && settings.api_keys?.elevenlabs) ||
            (settings.stt_provider === "gemini" && settings.api_keys?.google)) && (
            <p className="text-xs text-accent">
              {settings.stt_provider === "elevenlabs"
                ? "ElevenLabs"
                : settings.stt_provider === "gemini"
                ? "Gemini"
                : "Whisper"}{" "}
              transcription is ready — hold the mic button to talk
            </p>
          )}
        </section>

        {/* Document Knowledge Base (RAG) */}
        <section id="section-rag" className="space-y-3">
          <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
            <Database size={14} /> Document Knowledge Base
          </h2>
          <div className="space-y-2">
            {ragStatus ? (
              <div className="text-xs text-zinc-500 space-y-1">
                <p>
                  <span className="text-zinc-300">
                    {ragStatus.total_chunks} chunks
                  </span>{" "}
                  indexed from{" "}
                  <span className="text-zinc-300">
                    {ragStatus.source_files.length} source files
                  </span>
                </p>
                {ragStatus.last_ingested && (
                  <p>
                    Last indexed:{" "}
                    {new Date(ragStatus.last_ingested * 1000).toLocaleDateString()}
                  </p>
                )}
              </div>
            ) : (
              <p className="text-xs text-zinc-500">No documents indexed yet</p>
            )}
            <div className="space-y-1">
              <label htmlFor="docs-path" className="text-xs text-zinc-500">
                Path to documents folder
              </label>
              <div className="flex gap-2">
                <input
                  id="docs-path"
                  type="text"
                  value={docsPath}
                  onChange={(e) => setDocsPath(e.target.value)}
                  placeholder="e.g., C:\my-project\docs"
                  className="flex-1 bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-accent/50"
                />
                <button
                  type="button"
                  onClick={async () => {
                    // U16 audit: native folder picker so the user
                    // doesn't have to type a Windows path with
                    // backslashes by hand. open() returns null on
                    // cancel — keep the existing field unchanged
                    // in that case.
                    try {
                      const picked = await openDialog({
                        directory: true,
                        multiple: false,
                        title: "Choose your documents folder",
                      });
                      if (typeof picked === "string") {
                        setDocsPath(picked);
                      }
                    } catch (err) {
                      console.warn("[settings] folder picker failed:", err);
                    }
                  }}
                  className="px-3 py-2 text-xs rounded-lg bg-zinc-800 text-zinc-200 hover:bg-zinc-700 whitespace-nowrap"
                >
                  Browse…
                </button>
              </div>
            </div>
            {indexResult && (
              <p className="text-xs text-accent">{indexResult}</p>
            )}
            <div className="flex gap-2">
              <button
                onClick={async () => {
                  if (!docsPath.trim()) {
                    setIndexResult("Enter the path to your documents folder first");
                    return;
                  }
                  setIndexing(true);
                  setIndexResult(null);
                  try {
                    const count = await invoke<number>("ingest_all_documents", {
                      directory: docsPath.trim(),
                    });
                    setIndexResult(`Indexed ${count} chunks successfully`);
                    const status = await invoke<typeof ragStatus>(
                      "get_ingestion_status",
                    );
                    setRagStatus(status);
                  } catch (e) {
                    setIndexResult(`Indexing failed: ${e}`);
                  }
                  setIndexing(false);
                }}
                disabled={indexing}
                className="px-3 py-1.5 bg-accent/20 text-accent rounded-lg text-xs hover:bg-accent/30 disabled:opacity-30 flex items-center gap-1.5"
              >
                {indexing ? (
                  <>
                    <Loader2 size={12} className="animate-spin" />
                    Indexing...
                  </>
                ) : (
                  "Index Documents"
                )}
              </button>
              {ragStatus && ragStatus.total_chunks > 0 && (
                <button
                  onClick={async () => {
                    try {
                      await invoke("clear_doc_index");
                      setRagStatus({
                        total_chunks: 0,
                        source_files: [],
                        last_ingested: null,
                      });
                      setIndexResult("Index cleared");
                    } catch (e) {
                      setIndexResult(`Clear failed: ${e}`);
                    }
                  }}
                  disabled={indexing}
                  className="px-3 py-1.5 bg-zinc-800 text-zinc-300 rounded-lg text-xs hover:bg-zinc-700 disabled:opacity-30"
                >
                  Clear Index
                </button>
              )}
            </div>
            <p className="text-xs text-zinc-600">
              Requires an OpenAI API key for generating embeddings. RAG supplements
              static context with query-specific documentation.
            </p>
          </div>
        </section>

        {/* Claude Code + Wotch integration */}
        <section id="section-integrations" className="space-y-3">
          <ClaudeCodeIntegrationSection
            settings={settings}
            updateSettings={updateSettings}
          />
        </section>

        {/* Diagnostics — local-only crash + activity logging (O1).
            Two buttons: open the OS log directory in the file manager,
            and copy the last 5 MB of the active log to the clipboard
            for handing to support. PRINCIPLES.md §97 forbids
            error-reporting SDKs, so this is the user-driven equivalent. */}
        <section id="section-diagnostics" className="space-y-3 pt-2 border-t border-zinc-800/50">
          <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
            <Bug size={14} /> Diagnostics
          </h2>
          <p className="text-xs text-zinc-500">
            WorkBuddy keeps a local activity + crash log on your machine.
            No log data is uploaded automatically. If support asks for it,
            use these buttons.
          </p>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              onClick={async () => {
                try {
                  await invoke<string>("open_log_dir");
                } catch (err) {
                  console.error("[diagnostics] open_log_dir failed:", err);
                  alert(`Could not open log directory: ${err}`);
                }
              }}
              className="flex items-center gap-2 px-3 py-1.5 text-xs rounded-md bg-zinc-800 hover:bg-zinc-700 text-zinc-200 transition-colors"
            >
              <FolderOpen size={14} /> Open log directory
            </button>
            <button
              type="button"
              onClick={async () => {
                try {
                  const tail = await invoke<string>("copy_last_log_tail");
                  if (!tail) {
                    alert("No log content yet — nothing to copy.");
                    return;
                  }
                  await navigator.clipboard.writeText(tail);
                  alert(
                    `Copied ${tail.length.toLocaleString()} chars (≈${
                      Math.round(tail.length / 1024)
                    } KB) of log to clipboard.`
                  );
                } catch (err) {
                  console.error("[diagnostics] copy_last_log_tail failed:", err);
                  alert(`Could not copy log tail: ${err}`);
                }
              }}
              className="flex items-center gap-2 px-3 py-1.5 text-xs rounded-md bg-zinc-800 hover:bg-zinc-700 text-zinc-200 transition-colors"
            >
              <Copy size={14} /> Copy last 5 MB
            </button>
          </div>
        </section>

        {/* About */}
        <section id="section-about" className="space-y-3 pt-2 border-t border-zinc-800/50">
          <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
            <Info size={14} /> About
          </h2>
          <div className="space-y-1.5 text-xs text-zinc-500">
            <div className="flex items-center gap-2">
              <img
                src="/workbuddy-mark.svg"
                alt=""
                aria-hidden="true"
                className="h-6 w-6 rounded-sm"
              />
              <p>
                <span className="text-zinc-300 font-medium">
                  WorkBuddy
                </span>{" "}
                {appVersion && `v${appVersion}`}
              </p>
            </div>
            <p>
              Cross-platform AI desktop assistant. Inspired by{" "}
              <a
                href="#"
                onClick={(e) => {
                  e.preventDefault();
                  open("https://github.com/iamsrikanthnani/pluely").catch(() => {});
                }}
                className="text-accent hover:underline cursor-pointer"
              >
                pluely
              </a>{" "}
              and{" "}
              <a
                href="#"
                onClick={(e) => {
                  e.preventDefault();
                  open("https://github.com/farzaa/clicky").catch(() => {});
                }}
                className="text-accent hover:underline cursor-pointer"
              >
                Clicky
              </a>
              .
            </p>
            <div className="flex gap-3 pt-1">
              <a
                href="#"
                onClick={(e) => {
                  e.preventDefault();
                  open(
                    "https://github.com/Frostbite1536/WorkBuddy",
                  ).catch(() => {});
                }}
                className="text-accent hover:underline cursor-pointer"
              >
                GitHub
              </a>
              <a
                href="#"
                onClick={(e) => {
                  e.preventDefault();
                  open(
                    "https://github.com/Frostbite1536/WorkBuddy/blob/main/docs/TUTORIAL.md",
                  ).catch(() => {});
                }}
                className="text-accent hover:underline cursor-pointer"
              >
                Tutorial
              </a>
            </div>
            <p className="text-zinc-700 pt-1">Proprietary — all rights reserved</p>
          </div>
        </section>
      </div>
    </div>
  );
}
