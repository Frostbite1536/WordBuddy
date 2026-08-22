// content.js — DOM scanner + highlight injection
// Runs on matched pages. Scans visible interactive elements and sends
// them to WordBuddy via the background service worker. Polls for
// highlight commands and injects CSS overlays into the page.

(function () {
  'use strict';

  let scanActive = false;
  let highlightActive = false;

  // Mirrors popup toggles — populated on load and kept in sync via
  // chrome.storage.onChanged so the user doesn't need to reload tabs
  // when they change settings in the popup.
  let paused = false;
  let maskInputs = false;

  const MAX_ELEMENTS = 400; // Cap payload to stay well under the server's 1 MB body limit.

  // GitHub pages often contain sensitive content (private repo names, draft
  // PR comments, review text). Force form-field masking on regardless of the
  // user's global toggle — metadata only, never typed values.
  const HOST_IS_GITHUB = (() => {
    const h = window.location.hostname;
    return h === 'github.com' || h.endsWith('.github.com');
  })();

  // ── Settings sync ────────────────────────────────────────────────

  chrome.storage.local.get(['paused', 'maskInputs'], (cfg) => {
    paused = !!cfg.paused;
    maskInputs = !!cfg.maskInputs;
  });

  chrome.storage.onChanged.addListener((changes, area) => {
    if (area !== 'local') return;
    if (changes.paused) paused = !!changes.paused.newValue;
    if (changes.maskInputs) maskInputs = !!changes.maskInputs.newValue;
  });

  // ── Privacy helpers ──────────────────────────────────────────────

  // Strip query string and fragment from a URL so things like OAuth
  // tokens in callbacks, session ids, and tracking params don't leave
  // the page along with the link metadata.
  function safeUrl(raw) {
    if (!raw) return null;
    try {
      const u = new URL(raw, window.location.href);
      return `${u.origin}${u.pathname}`;
    } catch {
      return null;
    }
  }

  // ── Meta-tag scanner ─────────────────────────────────────────────

  // Read curriculum metadata from <meta name="wordbuddy-*"> tags.
  // The desktop app prefers these over OS window-title parsing because
  // window titles get truncated/rearranged by browsers and reordered by
  // tab managers, while page-author meta tags are authoritative.
  // Length caps are defensive: keep the payload bounded even if a page
  // ships oversized values.
  function scanMetaTags() {
    const meta = {};
    const nodes = document.querySelectorAll('meta[name^="wordbuddy-"]');
    for (const el of nodes) {
      const name = el.getAttribute('name');
      const content = el.getAttribute('content');
      if (!name || content == null) continue;
      const key = name.slice('wordbuddy-'.length).slice(0, 60);
      if (!key) continue;
      meta[key] = String(content).slice(0, 200);
    }
    return meta;
  }

  // ── DOM Scanner ──────────────────────────────────────────────────

  function scanVisibleElements() {
    const elements = [];
    const selectors = [
      'button', 'a', 'input', 'select', 'textarea',
      '[role="button"]', '[role="link"]', '[role="tab"]',
      'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
      'label', '[data-testid]',
    ].join(', ');

    const seen = new Set();
    let truncated = false;

    const nodes = document.querySelectorAll(selectors);
    for (const el of nodes) {
      if (elements.length >= MAX_ELEMENTS) {
        truncated = true;
        break;
      }
      if (seen.has(el)) continue;
      seen.add(el);

      const rect = el.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) continue;
      if (rect.bottom < 0 || rect.top > window.innerHeight) continue;
      if (rect.right < 0 || rect.left > window.innerWidth) continue;

      // Never capture password field values (sensitive data).
      if (el.tagName === 'INPUT' && el.type === 'password') continue;

      // When "Don't send form-field values" is on, skip the typed
      // value for input/textarea and fall back to label/placeholder
      // so the tutor still knows a field exists.
      const isFormField =
        el.tagName === 'INPUT' ||
        el.tagName === 'TEXTAREA' ||
        el.tagName === 'SELECT';
      const effectiveMask = maskInputs || HOST_IS_GITHUB;
      const fieldValue =
        isFormField && el.type !== 'hidden' && !effectiveMask ? el.value : null;

      const text = (
        el.textContent ||
        fieldValue ||
        el.placeholder ||
        ''
      )
        .trim()
        .replace(/\s+/g, ' ')
        .slice(0, 80);
      if (!text) continue;

      elements.push({
        tag: el.tagName.toLowerCase(),
        text,
        rect: {
          x: Math.round(rect.left),
          y: Math.round(rect.top),
          w: Math.round(rect.width),
          h: Math.round(rect.height),
        },
        type: el.type || null,
        href: safeUrl(el.href),
      });
    }

    if (truncated) {
      console.debug(
        `[WordBuddy ext] scan truncated at ${MAX_ELEMENTS} elements`,
      );
    }

    return elements;
  }

  // ── Highlight Injection ──────────────────────────────────────────

  function ensureStyles() {
    if (document.getElementById('wordbuddy-ext-styles')) return;
    const style = document.createElement('style');
    style.id = 'wordbuddy-ext-styles';
    style.textContent = `
      @keyframes wordbuddy-fade-in {
        from { opacity: 0; transform: scale(0.95); }
        to   { opacity: 1; transform: scale(1); }
      }
    `;
    document.head.appendChild(style);
  }

  // Coerce to a finite integer — defense-in-depth against a compromised
  // or malfunctioning server returning non-numeric rect fields that
  // would otherwise land in cssText as attacker-controlled CSS.
  function toInt(n) {
    const v = Number(n);
    return Number.isFinite(v) ? Math.round(v) : null;
  }

  function highlightElement(rect, label) {
    const x = toInt(rect && rect.x);
    const y = toInt(rect && rect.y);
    const w = toInt(rect && rect.w);
    const h = toInt(rect && rect.h);
    if (x === null || y === null || w === null || h === null) return;

    ensureStyles();

    const overlay = document.createElement('div');
    overlay.className = 'wordbuddy-ext-highlight';
    overlay.style.cssText = `
      position: fixed;
      left: ${x}px; top: ${y}px;
      width: ${w}px; height: ${h}px;
      border: 3px solid #10b981;
      border-radius: 8px;
      background: rgba(16, 185, 129, 0.1);
      z-index: 999999;
      pointer-events: none;
      animation: wordbuddy-fade-in 0.2s ease-out;
    `;

    if (label) {
      const pill = document.createElement('div');
      pill.textContent = String(label).slice(0, 80);
      pill.style.cssText = `
        position: absolute; top: -28px; left: 0;
        background: #09090b; color: #10b981;
        padding: 2px 8px; border-radius: 4px;
        font-size: 12px; white-space: nowrap;
        font-family: system-ui, -apple-system, sans-serif;
      `;
      overlay.appendChild(pill);
    }

    document.body.appendChild(overlay);

    // Auto-remove after 3 seconds with fade-out
    setTimeout(() => {
      overlay.style.transition = 'opacity 0.3s';
      overlay.style.opacity = '0';
      setTimeout(() => overlay.remove(), 300);
    }, 3000);
  }

  // ── Communication with Background Worker ─────────────────────────

  function pushScan() {
    if (paused) return;
    // Skip scans when the tab is hidden. Scanning a backgrounded tab
    // wastes CPU/battery and its elements won't match what WordBuddy
    // would see from the capture anyway.
    if (document.visibilityState !== 'visible') return;
    if (scanActive) return;
    scanActive = true;

    const elements = scanVisibleElements();
    chrome.runtime.sendMessage(
      {
        type: 'scan',
        data: {
          url: safeUrl(window.location.href),
          title: document.title,
          elements,
          meta: scanMetaTags(),
        },
      },
      (response) => {
        scanActive = false;
        if (chrome.runtime.lastError) return;
        if (response?.highlights?.length > 0) {
          response.highlights.forEach((entry) =>
            highlightElement(entry.rect, entry.label),
          );
        }
        // PLAN-02: the app rides the checking prefs (master switch +
        // excluded hosts) on the scan response so the checker script
        // gets them through chrome.storage without a new endpoint.
        if (response && (typeof response.checkingEnabled === 'boolean' || Array.isArray(response.excludedHosts))) {
          const prefs = {};
          if (typeof response.checkingEnabled === 'boolean') prefs.checkingEnabled = response.checkingEnabled;
          if (Array.isArray(response.excludedHosts)) prefs.excludedHosts = response.excludedHosts;
          chrome.storage.local.set(prefs);
        }
      },
    );
  }

  function pollHighlights() {
    if (paused) return;
    // Don't poll when the tab is hidden — highlights on a non-visible
    // tab would be invisible anyway and the service worker wake-ups
    // from a backgrounded tab are wasteful.
    if (document.visibilityState !== 'visible') return;
    if (highlightActive) return;
    highlightActive = true;

    chrome.runtime.sendMessage({ type: 'getHighlights' }, (response) => {
      highlightActive = false;
      if (chrome.runtime.lastError) return;
      if (response?.highlights?.length > 0) {
        response.highlights.forEach((entry) =>
          highlightElement(entry.rect, entry.label),
        );
      }
    });
  }

  // ── Lifecycle ────────────────────────────────────────────────────

  // Initial scan on page load
  pushScan();

  // Re-scan every 3 seconds to keep element data fresh.
  // Re-poll for highlight commands every 300ms for responsive feedback.
  // No explicit clearInterval() — content-script execution context is
  // torn down by the browser on navigation or extension reload, which
  // also kills these timers. Guarding with `paused`/visibility keeps
  // the hot path cheap when they would otherwise do nothing.
  setInterval(pushScan, 3000);
  setInterval(pollHighlights, 300);

  // When the tab becomes visible again after being hidden, the cached
  // data on the WordBuddy side may be stale (>3s old). Push a fresh
  // scan immediately so the next capture sees current elements.
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible') {
      pushScan();
    }
  });
})();
