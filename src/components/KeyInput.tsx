import { useEffect, useState } from "react";
import { Eye, EyeOff } from "lucide-react";

// Reusable masked credential input with show/hide toggle and
// trim-on-save semantics. U11 audit fix: ports the Onboarding
// pattern to every credential field in Settings so a copy-pasted
// key with a trailing newline / leading whitespace doesn't get
// persisted verbatim.

interface KeyInputProps {
  /** Current value held by parent state. */
  value: string;
  /** Called with the trimmed value on Save / blur. Parent decides
   *  whether to persist immediately or batch. */
  onSave: (trimmed: string) => void;
  /** Optional placeholder shown when the field is empty. */
  placeholder?: string;
  /** Aria label / id base. */
  id?: string;
  ariaLabel?: string;
  /** When true, the field is disabled and the eye toggle hides. */
  disabled?: boolean;
  /** Optional className for the wrapper. */
  className?: string;
}

// Trim + strip ALL embedded whitespace. API keys never contain
// internal whitespace; pasted-with-newline is the dominant failure
// mode the audit flagged.
export function cleanKey(raw: string): string {
  return raw.trim().replace(/\s+/g, "");
}

export function KeyInput({
  value,
  onSave,
  placeholder,
  id,
  ariaLabel,
  disabled,
  className,
}: KeyInputProps) {
  const [show, setShow] = useState(false);
  const [draft, setDraft] = useState(value);

  // Sync the local draft when the parent's `value` changes from
  // outside (a Withdraw that clears the key, a fresh load from
  // settings, etc.). Without this, `draft` is only initialized once
  // on mount and the input keeps showing stale text after a parent
  // reset. PR #33 P1 fix.
  useEffect(() => {
    setDraft(value);
  }, [value]);

  return (
    <div className={`relative ${className ?? ""}`}>
      <input
        id={id}
        type={show ? "text" : "password"}
        name={id ?? "credential"}
        autoComplete="off"
        spellCheck={false}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => {
          const cleaned = cleanKey(draft);
          if (cleaned !== value) {
            onSave(cleaned);
            setDraft(cleaned);
          }
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            const cleaned = cleanKey(draft);
            if (cleaned !== value) {
              onSave(cleaned);
              setDraft(cleaned);
            }
          }
        }}
        placeholder={placeholder}
        aria-label={ariaLabel}
        disabled={disabled}
        className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 pr-9 text-xs font-mono focus:outline-none focus:border-accent/50 disabled:opacity-50"
      />
      {!disabled && (
        <button
          type="button"
          onClick={() => setShow((s) => !s)}
          aria-label={show ? "Hide value" : "Show value"}
          title={show ? "Hide" : "Show"}
          className="absolute right-2 top-1/2 -translate-y-1/2 p-1 text-zinc-500 hover:text-zinc-300"
        >
          {show ? <EyeOff size={14} /> : <Eye size={14} />}
        </button>
      )}
    </div>
  );
}
