// Map raw provider/Tauri error strings into actionable, user-readable
// guidance. Without this, students see "Request failed: error trying
// to connect: dns error: failed to lookup address information" and
// have no idea what to fix. U5 audit.
//
// Caller should still attach the raw text in a "Details:" line for
// any case the mapper doesn't recognize — opaque errors are worse
// than unfriendly ones.

export function friendlyStreamError(raw: unknown): string {
  const msg = raw instanceof Error ? raw.message : String(raw);

  // 401/403 / explicit auth failures.
  if (/401|unauthorized|invalid api key|invalid_api_key|missing.*api.?key/i.test(msg)) {
    return (
      "Your API key was rejected. Open Settings → API Keys and check that the key for your selected provider is current. If the key looks right, the provider may have rotated or revoked it."
    );
  }

  // Rate limits.
  if (/429|rate.?limit/i.test(msg)) {
    return (
      "The provider rate-limited this request. Wait a moment and try again. If this keeps happening, your account's per-minute or per-day quota may be exhausted."
    );
  }

  // Quota / billing.
  if (/insufficient.*credit|billing|quota.*exceed|out of credit/i.test(msg)) {
    return (
      "Your provider account is out of credits or has hit its quota. Top up at the provider's billing page (Anthropic / OpenAI / Groq console) and try again."
    );
  }

  // 5xx upstream.
  if (/5\d\d|server error|internal server error|service unavailable/i.test(msg)) {
    return (
      "The provider returned a server error. This is usually transient — try again in a minute. If it persists, check the provider's status page."
    );
  }

  // Network errors (DNS, TLS, refused connection).
  if (
    /network|dns|name or service|failed to lookup|connection refused|connection reset|timeout|timed out|unreachable|tls|certificate/i.test(
      msg,
    )
  ) {
    return (
      "Network error reaching the provider. Check your internet connection. If you're on a corporate network or VPN, the provider's API host may be blocked."
    );
  }

  // Tauri / IPC errors.
  if (/tauri|ipc|invoke/i.test(msg)) {
    return (
      "The app's backend rejected this request. This is usually a configuration issue — try restarting WordBuddy. If it persists, check Settings → API Keys."
    );
  }

  // Default: surface the raw text so power users can debug.
  return `Something went wrong. Details: ${msg.slice(0, 300)}`;
}
