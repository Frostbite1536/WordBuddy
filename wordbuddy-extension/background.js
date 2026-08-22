// background.js — MV3 service worker
// Handles HTTP communication with WordBuddy's localhost server.
// Content scripts delegate all network requests here (they can't make
// cross-origin requests to localhost in MV3).

// Ports WordBuddy tries to bind to, in order. Must match the fallback
// list in extension.rs::start_extension_server.
const WORDBUDDY_PORTS = [19521, 19522, 19523];

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg.type === 'check') {
    handleCheck(msg.request)
      .then((body) => sendResponse({ ok: true, body }))
      .catch((err) => sendResponse({ ok: false, error: String(err && err.message || err) }));
    return true; // async sendResponse
  }
  if (msg.type === 'scan') {
    handleScan(msg.data).then(sendResponse).catch(() => sendResponse({ ok: false }));
    return true; // async response
  }
  if (msg.type === 'getHighlights') {
    fetchHighlights().then(sendResponse).catch(() => sendResponse({ highlights: [] }));
    return true;
  }
  if (msg.type === 'getConfig') {
    chrome.storage.local.get(['token', 'port', 'connected'], sendResponse);
    return true;
  }
});

async function getConfig() {
  return new Promise(resolve => {
    chrome.storage.local.get(['token', 'port'], resolve);
  });
}

// Port candidates to try, with the saved port first so successful
// connections stick. WordBuddy may have bound to a fallback port
// if the default was taken by another instance.
function portCandidates(savedPort) {
  const all = [savedPort || WORDBUDDY_PORTS[0], ...WORDBUDDY_PORTS];
  // Dedupe while preserving order
  return [...new Set(all)];
}

async function handleScan(data) {
  const config = await getConfig();
  const token = config.token;
  if (!token) {
    return { ok: false, error: 'No token configured' };
  }

  let lastError = null;
  for (const port of portCandidates(config.port)) {
    try {
      const resp = await fetch(`http://127.0.0.1:${port}/scan`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${token}`,
        },
        body: JSON.stringify(data),
      });

      if (!resp.ok) {
        // 401 means the server is reachable but the token is wrong —
        // trying other ports won't help. Report immediately.
        if (resp.status === 401) {
          await chrome.storage.local.set({ connected: false });
          return { ok: false, error: 'HTTP 401 (bad token)' };
        }
        lastError = `HTTP ${resp.status}`;
        continue;
      }

      // Success — remember the working port so the next call skips
      // the scan and connects on the first try.
      if (port !== config.port) {
        await chrome.storage.local.set({ port });
      }
      await chrome.storage.local.set({ connected: true });
      return await resp.json();
    } catch (e) {
      lastError = e.message || 'network error';
      // Connection refused / not listening — try next port
    }
  }

  await chrome.storage.local.set({ connected: false });
  return { ok: false, error: lastError || 'unreachable' };
}

// PLAN-02 /check relay. Body is the CONTRACTS CheckRequest JSON; the
// desktop app enforces auth + rate + host exclusion server-side too.
async function handleCheck(request) {
  const config = await getConfig();
  const token = config.token;
  if (!token) throw new Error('No token configured');

  let lastError = null;
  for (const port of portCandidates(config.port)) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/check`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify(request),
      });
      if (res.status === 429) {
        // Rate limited: skip this cycle, not an error (PLAN-02 risks).
        return { issues: [], styleCheckFailed: false, skipped: 'rate' };
      }
      if (res.status === 401) throw new Error('invalid token');
      if (!res.ok) throw new Error(`check failed: ${res.status}`);
      const body = await res.json();
      if (config.port !== port) await chrome.storage.local.set({ port });
      await chrome.storage.local.set({ connected: true });
      return body;
    } catch (e) {
      lastError = e;
    }
  }
  await chrome.storage.local.set({ connected: false });
  throw lastError || new Error('unreachable');
}

async function fetchHighlights() {
  const config = await getConfig();
  const token = config.token;
  if (!token) return { highlights: [] };

  // Highlights poll at 300ms — try the saved port first, and only on
  // connection refused fall through to the other candidates. This keeps
  // the hot path to a single fetch when things are healthy, but lets a
  // stale saved port recover without waiting for the next 3 s scan.
  const saved = config.port || WORDBUDDY_PORTS[0];
  const candidates = portCandidates(saved);

  for (const port of candidates) {
    try {
      const resp = await fetch(`http://127.0.0.1:${port}/highlight`, {
        headers: { 'Authorization': `Bearer ${token}` },
      });
      if (!resp.ok) {
        // Reachable but the server rejected — don't keep probing.
        return { highlights: [] };
      }
      if (port !== saved) {
        await chrome.storage.local.set({ port });
      }
      return await resp.json();
    } catch {
      // Connection refused — try next port.
    }
  }
  return { highlights: [] };
}
