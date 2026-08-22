// Helpers for opening external URLs from the WebView. The S4 audit
// flagged that any markdown link rendered from a model response —
// or from chat history loaded from SQLite — opens directly in the
// system browser via `open()` from @tauri-apps/plugin-shell, with no
// destination preview. A model-rendered `[Limitless](attacker.com)`
// would silently navigate the student to a phishing site styled
// like the live trading platform they're being trained to use.
//
// `confirmExternalLink` shows the full URL via window.confirm before
// the caller invokes `open()`. Hosts tied to the configured LLM
// providers bypass the prompt so normal navigation isn't friction-heavy.

// Apex domains where ANY subdomain is trusted (LLM provider consoles).
// Listing the apex only — putting a
// pre-existing subdomain like `www.limitless.exchange` here would
// have caused `evil.www.limitless.exchange` to also match the
// `endsWith("." + "www.limitless.exchange")` check (PR #32 P2
// audit).
const TRUSTED_APEX: ReadonlySet<string> = new Set([
  "limitless.exchange",
  "anthropic.com",
  "openai.com",
  "openrouter.ai",
  "ollama.com",
]);

// Hosts trusted only by EXACT match — no subdomain wildcard. Used
// for entries where the apex covers user-controlled subdomains
// (github.com → user.github.io is a different domain, but
// `pages.github.com` etc. could be subdomain-takeover targets) or
// where the canonical URL is itself a sub of a generic apex
// (`ai.google.dev`, `console.groq.com`).
const TRUSTED_EXACT: ReadonlySet<string> = new Set([
  "github.com",
  "www.github.com",
  "ai.google.dev",
  "console.groq.com",
]);

function isTrustedHost(host: string): boolean {
  if (TRUSTED_EXACT.has(host)) return true;
  if (TRUSTED_APEX.has(host)) return true;
  // Apex-only subdomain wildcard. Iterating TRUSTED_APEX (not the
  // EXACT set) means `pages.github.com` does NOT bypass the prompt
  // even though `github.com` is in EXACT.
  for (const apex of TRUSTED_APEX) {
    if (host.endsWith("." + apex)) return true;
  }
  return false;
}

/**
 * Returns true when the user has authorized opening `href` in the
 * system browser. For trusted provider hosts this returns true
 * silently; otherwise the user sees a confirm() with the full URL.
 */
export function confirmExternalLink(href: string): boolean {
  if (!href) return false;
  let parsed: URL;
  try {
    parsed = new URL(href);
  } catch {
    // Malformed URL — refuse rather than punt to the OS shell.
    return false;
  }
  if (parsed.protocol !== "https:" && parsed.protocol !== "http:") {
    // Block non-web schemes outright (mailto:, file:, javascript:, etc.).
    // The shell plugin's allowlist already gates these, but we add an
    // explicit no for clarity and to keep this the single decision point.
    return false;
  }
  if (isTrustedHost(parsed.host)) return true;
  // Render the FULL URL so the user can spot homoglyph attacks
  // (lіmіtless vs limitless) and hidden subdomains.
  const message =
    `Open this link in your browser?\n\n${href}\n\n` +
    `Host: ${parsed.host}\n\n` +
    `Tip: model-rendered links can disguise the destination. ` +
    `Decline if you didn't expect this URL.`;
  return window.confirm(message);
}
