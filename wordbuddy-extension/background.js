// background.js — MV3 service worker
// Handles HTTP communication with WordBuddy's localhost server.
// Content scripts delegate all network requests here (they can't make
// cross-origin requests to localhost in MV3).

// Ports WordBuddy tries to bind to, in order. Must match the fallback
// list in extension.rs::start_extension_server.
const WORDBUDDY_PORTS = [19521, 19522, 19523];

// App-supplied checking/exclusion preferences are ephemeral. Session storage
// is normally hidden from content scripts, so explicitly expose only this
// non-secret state to the extension's isolated-world scripts.
const sessionStorageReady = new Promise((resolve) => {
  chrome.storage.session.setAccessLevel(
    { accessLevel: 'TRUSTED_AND_UNTRUSTED_CONTEXTS' },
    () => {
      // Reading lastError is required to clear a benign failure on browsers
      // that do not expose session storage to content scripts.
      void chrome.runtime.lastError;
      resolve();
    },
  );
});

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  // Defense-in-depth: only accept messages from our own content scripts
  // or extension pages. (Other extensions can only reach onMessageExternal
  // listeners, of which we register none — this guard contains the blast
  // radius if one is ever introduced.)
  if (!sender || sender.id !== chrome.runtime.id || sender.tab?.incognito || !isRecord(msg)) return false;
  if (msg.type === 'check') {
    const request = sanitizeCheckRequest(msg.request, sender);
    if (!request) {
      sendResponse({ ok: false, error: 'Invalid check request' });
      return false;
    }
    handleCheck(request)
      .then((body) => sendResponse({ ok: true, body }))
      .catch((err) => sendResponse({ ok: false, error: String(err && err.message || err) }));
    return true; // async sendResponse
  }
  if (msg.type === 'scan') {
    if (!isValidScanPayload(msg.data)) {
      sendResponse({ ok: false, error: 'Invalid scan payload' });
      return false;
    }
    handleScan(msg.data).then(sendResponse).catch(() => sendResponse({ ok: false }));
    return true; // async response
  }
  if (msg.type === 'getHighlights') {
    fetchHighlights().then(sendResponse).catch(() => sendResponse({ highlights: [] }));
    return true;
  }
});

const MAX_CHECK_BYTES = 20_000;
const MAX_SCAN_ELEMENTS = 400;
const MAX_SCAN_TITLE_CHARS = 200;
const MAX_SCAN_META_ENTRIES = 50;
const UTF8_ENCODER = new TextEncoder();

function isRecord(value) {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}

function utf8ByteLength(value) {
  return UTF8_ENCODER.encode(value).length;
}

function senderHost(sender) {
  try {
    return sender.tab?.url ? new URL(sender.tab.url).hostname.toLowerCase() : null;
  } catch {
    return null;
  }
}

function sanitizeCheckRequest(request, sender) {
  if (!isRecord(request) || typeof request.text !== 'string' || utf8ByteLength(request.text) > MAX_CHECK_BYTES) return null;
  if (!isRecord(request.target) || request.target.kind !== 'browserHost') return null;
  const host = typeof request.target.host === 'string' ? request.target.host.toLowerCase() : '';
  if (!host || host.length > 253 || /[\s/\\]/.test(host)) return null;
  const actualHost = senderHost(sender);
  if (actualHost && actualHost !== host) return null;

  const goals = isRecord(request.goals) ? request.goals : {};
  return {
    text: request.text,
    surface: 'browser',
    target: { kind: 'browserHost', host },
    style_enabled: request.style_enabled === true,
    goals: {
      dialect: typeof goals.dialect === 'string' ? goals.dialect.slice(0, 64) : 'enUs',
      domain: typeof goals.domain === 'string' ? goals.domain.slice(0, 64) : 'General',
      formality: typeof goals.formality === 'string' ? goals.formality.slice(0, 64) : 'Neutral',
      audience: typeof goals.audience === 'string' ? goals.audience.slice(0, 64) : 'General',
      intent: typeof goals.intent === 'string' ? goals.intent.slice(0, 256) : null,
    },
  };
}

function isValidScanPayload(data) {
  if (!isRecord(data) || typeof data.url !== 'string' || data.url.length > 2_048) return false;
  if (typeof data.title !== 'string' || data.title.length > MAX_SCAN_TITLE_CHARS) return false;
  if (!Array.isArray(data.elements) || data.elements.length > MAX_SCAN_ELEMENTS) return false;
  if (!isRecord(data.meta) || Object.keys(data.meta).length > MAX_SCAN_META_ENTRIES) return false;
  try {
    const url = new URL(data.url);
    if (!/^https?:$/.test(url.protocol) || url.search || url.hash) return false;
  } catch {
    return false;
  }
  for (const el of data.elements) {
    if (!isRecord(el) || typeof el.tag !== 'string' || el.tag.length > 32) return false;
    if (typeof el.text !== 'string' || el.text.length > 80) return false;
    if (el.type !== null && (typeof el.type !== 'string' || el.type.length > 64)) return false;
    if (el.href !== null && (typeof el.href !== 'string' || el.href.length > 2_048)) return false;
    if (!isRecord(el.rect) || !['x', 'y', 'w', 'h'].every((key) => Number.isFinite(el.rect[key]))) return false;
  }
  for (const [key, value] of Object.entries(data.meta)) {
    if (!/^[a-zA-Z0-9_-]{1,60}$/.test(key) || typeof value !== 'string' || value.length > 200) return false;
  }
  return true;
}

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
  await sessionStorageReady;
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
      if (res.status === 401) {
        await chrome.storage.local.set({ connected: false });
        throw new Error('invalid token');
      }
      if (!res.ok) throw new Error(`check failed: ${res.status}`);
      const body = await res.json();
      if (config.port !== port) await chrome.storage.local.set({ port });
      await chrome.storage.local.set({ connected: true });
      return body;
    } catch (e) {
      lastError = e;
      if (e && e.message === 'invalid token') break;
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
        if (resp.status === 401) await chrome.storage.local.set({ connected: false });
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

// ── Dynamic site coverage (optional host permissions) ───────────────
//
// The manifest only auto-runs content scripts on the built-in matches.
// Sites the user grants via the popup's "Enable on this site" get their
// scripts injected here, at navigation commit. Granted origins are read
// from the permissions API — no second source of truth to keep in sync.

async function grantedForUrl(url) {
  try {
    const parsed = new URL(url);
    if (!/^https?:$/.test(parsed.protocol) || !parsed.hostname) return false;
    // Chrome permission match patterns deliberately omit ports. Using
    // URL.origin here makes dynamically granted localhost:3000-style sites
    // look ungranted after the first reload.
    return await chrome.permissions.contains({ origins: [`*://${parsed.hostname}/*`] });
  } catch {
    return false;
  }
}

async function ensureInjected(tabId) {
  try {
    const [probe] = await chrome.scripting.executeScript({
      target: { tabId },
      func: () => !!(window).__wbContentLoaded,
    });
    if (probe?.result) return;
    await chrome.scripting.executeScript({
      target: { tabId },
      files: ['content.js', 'checker.js'],
    });
  } catch {
    // Restricted page (chrome://, web store) or racing navigation.
  }
}

chrome.tabs.onUpdated.addListener((tabId, info, tab) => {
  if (info.status !== 'complete' || !tab.url || tab.incognito) return;
  grantedForUrl(tab.url).then((granted) => {
    if (granted) ensureInjected(tabId);
  });
});
