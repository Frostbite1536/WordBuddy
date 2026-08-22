// background.js — MV3 service worker
// Handles HTTP communication with WorkBuddy's localhost server.
// Content scripts delegate all network requests here (they can't make
// cross-origin requests to localhost in MV3).

// Ports WorkBuddy tries to bind to, in order. Must match the fallback
// list in extension.rs::start_extension_server.
const WORKBUDDY_PORTS = [19521, 19522, 19523];

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
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
// connections stick. WorkBuddy may have bound to a fallback port
// if the default was taken by another instance.
function portCandidates(savedPort) {
  const all = [savedPort || WORKBUDDY_PORTS[0], ...WORKBUDDY_PORTS];
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

async function fetchHighlights() {
  const config = await getConfig();
  const token = config.token;
  if (!token) return { highlights: [] };

  // Highlights poll at 300ms — try the saved port first, and only on
  // connection refused fall through to the other candidates. This keeps
  // the hot path to a single fetch when things are healthy, but lets a
  // stale saved port recover without waiting for the next 3 s scan.
  const saved = config.port || WORKBUDDY_PORTS[0];
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
