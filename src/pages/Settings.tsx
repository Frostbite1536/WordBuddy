import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import {
  ArrowLeft,
  BookOpen,
  Key,
  Monitor,
  Loader2,
  CheckCircle,
  XCircle,
  Cpu,
  Info,
  Globe,
  Accessibility,
  Eye,
  EyeOff,
  FolderOpen,
  Copy,
  Bug,
} from "lucide-react";
import { KeyInput, cleanKey } from "../components/KeyInput";
import { open } from "@tauri-apps/plugin-shell";
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
        Connects WordBuddy to your browser for in-page writing checks.
      </p>

      <div className="flex items-center gap-2">
        <div className={`w-2 h-2 rounded-full ${status?.connected ? "bg-emerald-500" : "bg-zinc-600"}`} />
        <span className={`text-xs ${status?.connected ? "text-emerald-400" : "text-zinc-500"}`}>
          {status?.connected
            ? `Connected \u2014 ${status.page_title || "page"}`
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
              <code className="text-zinc-400">wordbuddy-extension/</code> via
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

      {/* Highlight routing through the extension */}
      <div className="flex items-start justify-between gap-3 pt-2 border-t border-zinc-800/50">
        <div>
          <h3 className="text-xs font-semibold text-zinc-300">
            Use extension for highlights
          </h3>
          <p className="text-xs text-zinc-600">
            When on, highlight commands paint an in-page rectangle via the
            extension on browser pages (scrolls with the page).
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
  const [validating, setValidating] = useState(false);
  const [validationResult, setValidationResult] = useState<boolean | null>(null);
  const [appVersion, setAppVersion] = useState("");

  // Load available providers and app version
  useEffect(() => {
    invoke<ProviderInfo[]>("list_providers")
      .then(setProviders)
      .catch(() => {});
    getVersion().then(setAppVersion).catch(() => {});
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

  const handleProviderChange = (newProvider: string) => {
    const provider = providers.find((p) => p.id === newProvider);
    const defaultModel = provider?.models[0]?.id || "";
    updateSettings({ provider: newProvider, model: defaultModel });
  };

  // Jump-nav targets so the sections aren't a single long scroll.
  // Anchored to section ids added below; clicking a chip uses
  // scrollIntoView + smooth scroll inside the page's overflow-y-auto.
  const sectionAnchors: Array<{ id: string; label: string }> = [
    { id: "section-ai", label: "AI" },
    { id: "section-keys", label: "Keys" },
    { id: "section-extension", label: "Extension" },
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

        {/* Section jump-nav. Sticks to the top of the scroll container
            so the user can hop between sections without a scroll hunt. */}
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

        {/* Accessibility-powered UI Detection (used by native-field
            monitoring in PLAN-03) */}
        <section className="space-y-3">
          <div className="flex items-center justify-between">
            <div>
              <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
                <Accessibility size={14} /> Accessibility Detection
              </h2>
              <p className="text-xs text-zinc-600">
                Read UI element names + positions from the foreground window's
                accessibility tree so WordBuddy can detect the focused text
                field in native apps. Works best in IDEs, terminals, and
                Electron apps. Data stays local.
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

        {/* Browser Extension */}
        <section id="section-extension" className="space-y-3">
          <ExtensionSection settings={settings} updateSettings={updateSettings} />
        </section>

        {/* Diagnostics — local-only crash + activity logging (O1).
            Two buttons: open the OS log directory in the file manager,
            and copy the last 5 MB of the active log to the clipboard
            for handing to support. No error-reporting SDKs — this is
            the user-driven equivalent. */}
        <section id="section-diagnostics" className="space-y-3 pt-2 border-t border-zinc-800/50">
          <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
            <Bug size={14} /> Diagnostics
          </h2>
          <p className="text-xs text-zinc-500">
            WordBuddy keeps a local activity + crash log on your machine.
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
                src="/wordbuddy-mark.svg"
                alt=""
                aria-hidden="true"
                className="h-6 w-6 rounded-sm"
              />
              <p>
                <span className="text-zinc-300 font-medium">
                  WordBuddy
                </span>{" "}
                {appVersion && `v${appVersion}`}
              </p>
            </div>
            <p>Privacy-first system-wide writing assistant.</p>
            <p className="text-zinc-700 pt-1">Proprietary — all rights reserved</p>
          </div>
        </section>
      </div>
    </div>
  );
}
