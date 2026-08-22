import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Briefcase,
  Key,
  CheckCircle,
  XCircle,
  Loader2,
  ArrowRight,
  Monitor,
  Keyboard,
  Eye,
  EyeOff,
} from "lucide-react";
import { open } from "@tauri-apps/plugin-shell";
import { useApp } from "../contexts/app.context";

type Step = "welcome" | "api_key" | "shortcuts" | "ready";

const ONBOARDING_HEIGHT = 520;
const COLLAPSED_HEIGHT = 54;

export default function Onboarding() {
  const { updateSettings, setIsOnboarded, setCurrentPage } = useApp();
  const [step, setStep] = useState<Step>("welcome");

  const finishedRef = useRef(false);

  // Expand window for onboarding — cleanup only fires if user didn't finish
  useEffect(() => {
    invoke("set_window_height", { height: ONBOARDING_HEIGHT }).catch(() => {});
    return () => {
      if (!finishedRef.current) {
        invoke("set_window_height", { height: COLLAPSED_HEIGHT }).catch(() => {});
      }
    };
  }, []);
  const [apiKey, setApiKey] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [validating, setValidating] = useState(false);
  const [valid, setValid] = useState<boolean | null>(null);
  // Actionable error text (U3 audit). null = no error displayed.
  const [validationError, setValidationError] = useState<string | null>(null);
  // Tracks whether the user reached the shortcuts step by validating a
  // key (true) or by skipping (false). Used to gate the final "ready"
  // step's behavior — the bare-skip path warns the user that no
  // provider is configured rather than silently dropping them into
  // chat with no working key (U4 audit).
  const [skippedKey, setSkippedKey] = useState(false);

  // Trim + strip embedded whitespace before saving. Pasted keys
  // commonly carry a trailing newline or leading space; `password`
  // inputs hide the garbage so the user can't see why validation
  // fails. U11 audit.
  const cleanKey = (raw: string) => raw.trim().replace(/\s+/g, "");

  const handleValidateKey = async () => {
    setValidating(true);
    setValid(null);
    setValidationError(null);
    const trimmed = cleanKey(apiKey);
    if (!trimmed) {
      setValid(false);
      setValidationError("Please paste your API key first.");
      setValidating(false);
      return;
    }
    try {
      const result = await invoke<boolean>("validate_api_key", {
        key: trimmed,
      });
      setValid(result);
      if (!result) {
        // Backend returns false for any non-2xx — give the student a
        // realistic taxonomy. U3 audit.
        setValidationError(
          "The provider rejected this key. Common causes: typo, key was rotated, account out of credits, or a corporate firewall is blocking api.anthropic.com.",
        );
        setValidating(false);
        return;
      }
      // Persistence has its own error path (U12 audit). A successful
      // validate followed by a failed set_api_key (disk full,
      // permission denied) used to silently mark `valid = false`,
      // misleading the student into thinking the key was bad.
      try {
        await invoke("set_api_key", { service: "anthropic", key: trimmed });
        setApiKey(trimmed);
      } catch (saveErr) {
        setValid(false);
        setValidationError(
          `The key validated but couldn't be saved: ${
            saveErr instanceof Error ? saveErr.message : String(saveErr)
          }. Check that WorkBuddy can write to its config directory.`,
        );
      }
    } catch (e) {
      setValid(false);
      const raw = e instanceof Error ? e.message : String(e);
      if (/network|timeout|connect|dns/i.test(raw)) {
        setValidationError(
          "Couldn't reach the provider to validate. Check your internet connection — corporate firewalls sometimes block api.anthropic.com.",
        );
      } else if (/429|rate.?limit/i.test(raw)) {
        setValidationError(
          "The provider rate-limited the validation request. Wait a moment and try again.",
        );
      } else {
        setValidationError(`Validation error: ${raw.slice(0, 200)}`);
      }
    }
    setValidating(false);
  };

  const handleFinish = async () => {
    finishedRef.current = true;
    // Collapse window back to bar height before transitioning
    await invoke("set_window_height", { height: COLLAPSED_HEIGHT }).catch(() => {});
    // Only persist the Anthropic key when it's actually valid. On the
    // "Skip — I'll use a different provider" path the key is never
    // validated; passing a half-typed value here would save garbage.
    if (valid && apiKey) {
      // Persist via set_api_key as a belt-and-suspenders write
      // (handleValidateKey already wrote it on success, but the user
      // could have re-typed since). Failure surfaces in console
      // because finishHandler is fire-and-forget by then; the
      // settings panel will reflect the actual on-disk state.
      try {
        await invoke("set_api_key", {
          service: "anthropic",
          key: cleanKey(apiKey),
        });
      } catch (e) {
        console.warn("[onboarding] final set_api_key failed:", e);
      }
    }
    setIsOnboarded(true);
    setCurrentPage("chat");
  };

  const handleSkipKey = () => {
    // U7 audit: warn before discarding a typed-but-unvalidated key.
    if (apiKey.trim() && valid !== true) {
      const ok = window.confirm(
        "You typed a key but didn't validate it. Skipping will discard it. Continue anyway?",
      );
      if (!ok) return;
      setApiKey("");
    }
    setSkippedKey(true);
    setStep("shortcuts");
  };

  return (
    <div className="h-full bg-background-primary flex items-center justify-center p-6 overflow-y-auto">
      <div className="max-w-md w-full space-y-6">
        {step === "welcome" && (
          <>
            <div className="text-center space-y-4">
              <div className="inline-flex p-4 rounded-2xl bg-accent/10">
                <Briefcase size={48} className="text-accent" />
              </div>
              <h1 className="text-2xl font-heading font-bold text-white">
                Welcome to WordBuddy
              </h1>
              <p className="text-zinc-400 text-sm leading-relaxed">
                Your AI writing assistant. WordBuddy answers questions,
                helps you draft and edit text, and — in the browser —
                checks your writing as you type.
              </p>
            </div>
            <button
              onClick={() => setStep("api_key")}
              className="w-full py-2.5 bg-accent text-white rounded-lg font-medium flex items-center justify-center gap-2 hover:bg-accent-hover transition-colors"
            >
              Get Started <ArrowRight size={16} />
            </button>
          </>
        )}

        {step === "api_key" && (
          <>
            <div className="space-y-2">
              <div className="flex items-center gap-2 text-accent">
                <Key size={20} />
                <h2 className="text-lg font-heading font-semibold">
                  Connect to Claude
                </h2>
              </div>
              <p className="text-zinc-400 text-sm">
                WordBuddy uses your chosen LLM provider to answer questions
                and help you write. Enter your Anthropic API key to get
                started.
              </p>
              <button
                type="button"
                // U2 audit: was a non-button anchor with href="#" and a
                // bare catch — silent dead-end if shell.open() failed.
                // Now: real button semantics; on open() failure, copy
                // the URL so the student has a recovery path.
                onClick={() => {
                  const url = "https://console.anthropic.com/settings/keys";
                  open(url).catch(() => {
                    if (navigator.clipboard) {
                      navigator.clipboard
                        .writeText(url)
                        .then(() =>
                          alert(
                            "Couldn't open your browser. Link copied — paste it manually.",
                          ),
                        )
                        .catch(() => alert(`Open this URL manually: ${url}`));
                    } else {
                      alert(`Open this URL manually: ${url}`);
                    }
                  });
                }}
                className="text-xs text-accent hover:underline cursor-pointer"
              >
                Get an API key at console.anthropic.com
              </button>
            </div>
            <div className="space-y-3">
              <div className="relative">
                <input
                  type={showApiKey ? "text" : "password"}
                  value={apiKey}
                  onChange={(e) => {
                    setApiKey(e.target.value);
                    setValid(null);
                    setValidationError(null);
                  }}
                  placeholder="sk-ant-api03-..."
                  aria-invalid={valid === false}
                  aria-describedby={validationError ? "key-error" : undefined}
                  className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2.5 pr-10 text-sm focus:outline-none focus:border-accent/50 font-mono"
                />
                <button
                  type="button"
                  onClick={() => setShowApiKey((s) => !s)}
                  aria-label={showApiKey ? "Hide API key" : "Show API key"}
                  title={showApiKey ? "Hide" : "Show"}
                  className="absolute right-2 top-1/2 -translate-y-1/2 p-1 text-zinc-500 hover:text-zinc-300"
                >
                  {showApiKey ? <EyeOff size={14} /> : <Eye size={14} />}
                </button>
              </div>
              <button
                onClick={handleValidateKey}
                disabled={!apiKey || validating}
                className="w-full py-2.5 bg-accent/20 text-accent rounded-lg text-sm font-medium hover:bg-accent/30 disabled:opacity-30 flex items-center justify-center gap-2"
              >
                {validating ? (
                  <><Loader2 size={16} className="animate-spin" /> Validating...</>
                ) : valid === true ? (
                  <><CheckCircle size={16} /> Key is valid</>
                ) : valid === false ? (
                  <><XCircle size={16} /> Validation failed</>
                ) : (
                  "Validate Key"
                )}
              </button>
              {validationError && (
                <p
                  id="key-error"
                  role="alert"
                  aria-live="polite"
                  className="text-[11px] text-red-400 bg-red-950/30 border border-red-900/40 rounded-md p-2"
                >
                  {validationError}
                </p>
              )}
            </div>
            {valid && (
              <button
                onClick={() => {
                  setSkippedKey(false);
                  setStep("shortcuts");
                }}
                className="w-full py-2.5 bg-accent text-white rounded-lg font-medium flex items-center justify-center gap-2 hover:bg-accent-hover"
              >
                Continue <ArrowRight size={16} />
              </button>
            )}
            <button
              onClick={handleSkipKey}
              className="w-full py-1.5 text-zinc-500 text-xs hover:text-zinc-300 transition-colors"
            >
              Skip — I'll use a different provider (OpenAI, Groq, Ollama, etc.)
            </button>
          </>
        )}

        {step === "shortcuts" && (
          <>
            <div className="space-y-2">
              <div className="flex items-center gap-2 text-accent">
                <Keyboard size={20} />
                <h2 className="text-lg font-heading font-semibold">
                  Keyboard Shortcuts
                </h2>
              </div>
              <p className="text-zinc-400 text-sm">
                These shortcuts work from any application.
              </p>
            </div>
            <div className="space-y-2">
              {[
                { keys: "Ctrl + Shift + S", action: "Show / hide WorkBuddy" },
                { keys: "Ctrl + Space", action: "Push-to-talk" },
                { keys: "Ctrl + Shift + X", action: "Take screenshot" },
              ].map((s) => (
                <div
                  key={s.keys}
                  className="flex items-center justify-between p-3 rounded-lg bg-zinc-900 border border-zinc-800"
                >
                  <span className="text-sm text-zinc-300">{s.action}</span>
                  <kbd className="text-xs font-mono bg-zinc-800 px-2 py-1 rounded text-zinc-400">
                    {s.keys}
                  </kbd>
                </div>
              ))}
            </div>
            <button
              onClick={() => setStep("ready")}
              className="w-full py-2.5 bg-accent text-white rounded-lg font-medium flex items-center justify-center gap-2 hover:bg-accent-hover"
            >
              Continue <ArrowRight size={16} />
            </button>
          </>
        )}

        {step === "ready" && (
          <>
            <div className="text-center space-y-4">
              <div className="inline-flex p-4 rounded-2xl bg-accent/10">
                <CheckCircle size={48} className="text-accent" />
              </div>
              <h1 className="text-2xl font-heading font-bold text-white">
                You're Ready!
              </h1>
              <p className="text-zinc-400 text-sm leading-relaxed">
                WordBuddy will float at the top of your screen. Type a question
                or press <kbd className="font-mono bg-zinc-800 px-1.5 py-0.5 rounded text-xs">Ctrl+Shift+S</kbd> to
                toggle visibility.
              </p>
              {/* U4 audit: warn the user when they're about to land in
                  the chat with no provider configured. Without this
                  they hit a cryptic "missing x-api-key" error on
                  their first question. */}
              {skippedKey && !valid && (
                <div
                  role="alert"
                  className="text-left text-[11px] text-amber-300 bg-amber-950/30 border border-amber-900/40 rounded-md p-3 space-y-1"
                >
                  <div className="font-medium">No provider configured yet.</div>
                  <p>
                    You skipped the API key step. Open Settings (gear icon
                    in the chat bar) and configure either an Anthropic /
                    OpenAI / Groq / OpenRouter key, or point at a local
                    Ollama instance, before asking your first question.
                  </p>
                </div>
              )}
            </div>
            <button
              onClick={handleFinish}
              className="w-full py-2.5 bg-accent text-white rounded-lg font-medium hover:bg-accent-hover transition-colors"
            >
              Get to Work
            </button>
          </>
        )}

        {/* Step indicator */}
        <div className="flex justify-center gap-1.5">
          {(["welcome", "api_key", "shortcuts", "ready"] as Step[]).map((s) => (
            <div
              key={s}
              className={`w-2 h-2 rounded-full transition-colors ${
                s === step ? "bg-accent" : "bg-zinc-700"
              }`}
            />
          ))}
        </div>
      </div>
    </div>
  );
}
