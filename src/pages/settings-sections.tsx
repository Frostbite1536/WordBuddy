import { Zap, BookOpen } from "lucide-react";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Toggle } from "../components/Toggle";
import { useApp } from "../contexts/app.context";

interface Snippet {
  trigger: string;
  body: string;
  cursor_offset: number;
}

function PersonalDictionarySection() {
  const { settings, updateSettings } = useApp();
  const [draft, setDraft] = useState("");
  const [testText, setTestText] = useState("Kubernetesy deploy");
  const [testResult, setTestResult] = useState<string | null>(null);

  const addWords = (words: string[]) => {
    const clean = words.map((w) => w.trim()).filter(Boolean);
    const merged = [...new Set([...settings.personal_dictionary, ...clean])];
    updateSettings({ personal_dictionary: merged });
  };

  return (
    <section className="space-y-3">
      <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
        <BookOpen size={14} /> Personal dictionary
      </h2>
      <p className="text-xs text-zinc-600">
        Words the checker should accept (product names, jargon). Takes
        effect on the next check — no restart.
      </p>
      <div className="flex gap-2">
        <input
          type="text"
          name="dictionary-word"
          autoComplete="off"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="Add a word…"
          aria-label="Add dictionary word"
          className="flex-1 bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:border-accent/50"
        />
        <button
          onClick={() => {
            if (draft.trim()) {
              addWords([draft]);
              setDraft("");
            }
          }}
          className="px-3 py-1.5 bg-accent/20 text-accent rounded-lg text-xs hover:bg-accent/30"
        >
          Add
        </button>
        <button
          onClick={async () => {
            try {
              const clip = await navigator.clipboard.readText();
              addWords(clip.split(/\r?\n/));
            } catch {
              // clipboard denied
            }
          }}
          title="Import words from clipboard (one per line)"
          className="px-3 py-1.5 bg-zinc-800 text-zinc-200 rounded-lg text-xs hover:bg-zinc-700"
        >
          Import from clipboard
        </button>
      </div>
      {settings.personal_dictionary.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {settings.personal_dictionary.map((word) => (
            <span key={word} className="flex items-center gap-1 px-2 py-0.5 rounded-full bg-zinc-800 text-[11px] text-zinc-300">
              {word}
              <button
                onClick={() =>
                  updateSettings({
                    personal_dictionary: settings.personal_dictionary.filter((w) => w !== word),
                  })
                }
                aria-label={`Remove ${word}`}
                className="text-zinc-500 hover:text-red-400"
              >
                ×
              </button>
            </span>
          ))}
        </div>
      )}
      <div className="space-y-1 pt-1">
        <label htmlFor="dictionary-test" className="text-xs text-zinc-500">
          Test box
        </label>
        <input
          id="dictionary-test"
          type="text"
          name="dictionary-test"
          autoComplete="off"
          value={testText}
          onChange={(e) => setTestText(e.target.value)}
          onBlur={() => {
            void invoke<{ issues: unknown[] }>("check_text_command", {
              request: {
                text: testText,
                surface: "palette",
                target: { kind: "browserHost", host: "settings-test" },
              },
            })
              .then((r) => {
                const correctness = r.issues.length;
                setTestResult(
                  `${correctness} issue(s) in "${testText}"` +
                    (correctness === 0 ? " — accepted ✓" : ""),
                );
              })
              .catch(() => setTestResult("check failed"));
          }}
          className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:border-accent/50"
        />
        {testResult && (
          <p className="text-[11px] text-accent" aria-live="polite">
            {testResult}
          </p>
        )}
      </div>
    </section>
  );
}

interface StyleRule {
  find: string;
  replace: string;
  case_sensitive: boolean;
}

function StyleGuideSection() {
  const { settings, updateSettings } = useApp();
  const rules = (settings as unknown as { style_rules?: StyleRule[] }).style_rules ?? [];
  const setRules = (r: StyleRule[]) => updateSettings({ style_rules: r });
  const [find, setFind] = useState("");
  const [replace, setReplace] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);

  return (
    <section className="space-y-3">
      <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
        <BookOpen size={14} /> Style guide
      </h2>
      <p className="text-xs text-zinc-600">
        Ordered replacement pairs flagged as suggestions (e.g.
        &quot;utilize → use&quot;).
      </p>
      <div className="flex flex-wrap gap-2 items-center">
        <input
          type="text"
          name="style-rule-find"
          autoComplete="off"
          value={find}
          onChange={(e) => setFind(e.target.value)}
          placeholder="Find text…"
          aria-label="Style rule find"
          className="flex-1 min-w-[120px] bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm"
        />
        <span className="text-zinc-600">→</span>
        <input
          type="text"
          name="style-rule-replacement"
          autoComplete="off"
          value={replace}
          onChange={(e) => setReplace(e.target.value)}
          placeholder="Replace with…"
          aria-label="Style rule replace"
          className="flex-1 min-w-[120px] bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm"
        />
        <label className="flex items-center gap-1 text-xs text-zinc-400">
          <input
            type="checkbox"
            name="style-rule-case-sensitive"
            checked={caseSensitive}
            onChange={(e) => setCaseSensitive(e.target.checked)}
          />
          Aa
        </label>
        <button
          onClick={() => {
            if (find.trim() && replace.trim()) {
              setRules([
                ...rules,
                { find: find.trim(), replace: replace.trim(), case_sensitive: caseSensitive },
              ]);
              setFind("");
              setReplace("");
            }
          }}
          className="px-3 py-1.5 bg-accent/20 text-accent rounded-lg text-xs hover:bg-accent/30"
        >
          Add rule
        </button>
      </div>
      {rules.map((r, i) => (
        <div key={i} className="flex justify-between text-xs bg-zinc-900 rounded-lg px-3 py-1.5">
          <span>
            <code className="text-zinc-300">{r.find}</code> →{" "}
            <code className="text-accent">{r.replace}</code>
            {r.case_sensitive && <span className="ml-1 text-zinc-600">(Aa)</span>}
          </span>
          <button
            onClick={() => setRules(rules.filter((_, j) => j !== i))}
            aria-label={`Remove rule ${r.find}`}
            className="text-zinc-600 hover:text-red-400"
          >
            ×
          </button>
        </div>
      ))}
      <div className="flex gap-2">
        <button
          onClick={async () => {
            try {
              const clip = await navigator.clipboard.readText();
              const parsed: StyleRule[] = JSON.parse(clip);
              if (Array.isArray(parsed)) setRules([...rules, ...parsed]);
            } catch {
              // invalid import
            }
          }}
          className="text-[11px] text-zinc-500 hover:text-zinc-300"
        >
          Import JSON from clipboard
        </button>
        <button
          onClick={() => void navigator.clipboard.writeText(JSON.stringify(rules, null, 2))}
          className="text-[11px] text-zinc-500 hover:text-zinc-300"
        >
          Copy JSON
        </button>
      </div>
    </section>
  );
}

declare global {
  interface Window {
    __wbSnippetTest?: (typed: string) => Promise<{ expanded: string } | null>;
  }
}

function SnippetsSection() {
  const { settings, updateSettings } = useApp();
  const snippets = (settings as unknown as { snippets?: Snippet[] }).snippets ?? [];
  const [trigger, setTrigger] = useState("");
  const [body, setBody] = useState("");
  const [testTyped, setTestTyped] = useState(";meet");
  const [testExpanded, setTestExpanded] = useState<string | null>(null);

  const startHook = () =>
    invoke("snippet_hook_start").catch(() => {});
  const stopHook = () =>
    invoke("snippet_hook_stop").catch(() => {});

  return (
    <section className="space-y-3">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
            <Zap size={14} /> Snippets (text expansion)
          </h2>
          <p className="text-xs text-zinc-600">
            Type a trigger like <code>;addr</code> anywhere and it expands to
            your snippet. Uses a keyboard hook — OFF by default; the system
            self-disables it if anything misbehaves.
          </p>
        </div>
        <Toggle
          checked={settings.snippets_enabled}
          onChange={() => {
            const next = !settings.snippets_enabled;
            updateSettings({ snippets_enabled: next });
            if (next) void startHook();
            else void stopHook();
          }}
          label="Snippets enabled"
        />
      </div>

      {settings.snippets_enabled && (
        <>
          <div className="space-y-2">
            {snippets.map((snip, i) => (
              <div key={i} className="flex items-center justify-between text-xs bg-zinc-900 rounded-lg px-3 py-1.5">
                <span>
                  <code className="text-accent">{snip.trigger}</code> → {snip.body.slice(0, 40)}
                  {snip.body.length > 40 ? "…" : ""}
                </span>
                <button
                  onClick={() =>
                    updateSettings({
                      snippets: snippets.filter((_, j) => j !== i),
                    })
                  }
                  aria-label={`Remove snippet ${snip.trigger}`}
                  className="text-zinc-600 hover:text-red-400"
                >
                  ×
                </button>
              </div>
            ))}
          </div>
          <div className="flex gap-2">
            <input
              type="text"
              name="snippet-trigger"
              autoComplete="off"
              value={trigger}
              onChange={(e) => setTrigger(e.target.value)}
              placeholder="Trigger, e.g. ;addr…"
              aria-label="Snippet trigger"
              className="w-32 bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm"
            />
            <input
              type="text"
              name="snippet-body"
              autoComplete="off"
              value={body}
              onChange={(e) => setBody(e.target.value)}
              placeholder="Body ($CURSOR$ marks caret spot)…"
              aria-label="Snippet body"
              className="flex-1 bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm"
            />
            <button
              onClick={() => {
                if (!trigger.startsWith(";") || !body.trim()) return;
                updateSettings({
                  snippets: [
                    ...snippets,
                    { trigger: trigger.trim(), body, cursor_offset: 0 },
                  ],
                });
                setTrigger("");
                setBody("");
                void startHook(); // refresh triggers
              }}
              className="px-3 py-1.5 bg-accent/20 text-accent rounded-lg text-xs hover:bg-accent/30 whitespace-nowrap"
            >
              Add
            </button>
          </div>
          <div className="space-y-1 pt-2 border-t border-zinc-800/50">
            <label htmlFor="snippet-test" className="text-xs text-zinc-500">
              Test expansion locally (no hook involved)
            </label>
            <div className="flex gap-2 items-center">
              <input
                id="snippet-test"
                type="text"
                name="snippet-test"
                autoComplete="off"
                value={testTyped}
                onChange={async (e) => {
                  setTestTyped(e.target.value);
                  try {
                    const res = await invoke<{ expanded: string } | null>(
                      "snippet_test",
                      { typed: e.target.value },
                    );
                    setTestExpanded(res?.expanded ?? null);
                  } catch {
                    setTestExpanded(null);
                  }
                }}
                aria-label="Snippet test input"
                className="flex-1 bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm"
              />
              {testExpanded && (
                <span className="text-[11px] text-emerald-400">→ {testExpanded}</span>
              )}
            </div>
          </div>
          <p className="text-[10px] text-zinc-600">
            Expansion is skipped in terminals and IDEs by default.
          </p>
        </>
      )}
    </section>
  );
}

export { PersonalDictionarySection, StyleGuideSection, SnippetsSection };
