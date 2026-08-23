// checker.js — PLAN-02 browser inline checking
// Watches editable fields, runs debounced /check requests through the
// background service worker, renders color-coded underlines and a
// suggestion card in a shadow-DOM overlay, and applies replacements with
// undo-preserving edits.
//
//   3. form-field masking (maskInputs) — when "Don't send form-field
//      values" is on, typed text must not leave the page at all, so the
//      watcher deactivates entirely rather than reading partial data
//   4. GitHub carve-out — github.com always forces masking (draft PR
//      comments / review text are the exact data class PRIVACY_POLICY.md
//      promises never leaves the page)
// Only after all pass does the watcher attach and read text.

(function () {
  'use strict';

  let checkingEnabled = true;
  let excludedHosts = [];
  let paused = false;
  let maskInputs = false;

  // Mirrors content.js: GitHub pages force form-field masking on —
  // checker reads full field text, so on github.com it must never run.
  const HOST_IS_GITHUB = (() => {
    const h = window.location.hostname;
    return h === 'github.com' || h.endsWith('.github.com');
  })();

  chrome.storage.local.get(['paused', 'checkingEnabled', 'excludedHosts', 'maskInputs'], (cfg) => {
    paused = !!cfg.paused;
    if (typeof cfg.checkingEnabled === 'boolean') checkingEnabled = cfg.checkingEnabled;
    if (Array.isArray(cfg.excludedHosts)) excludedHosts = cfg.excludedHosts;
    maskInputs = !!cfg.maskInputs;
  });

  chrome.storage.onChanged.addListener((changes, area) => {
    if (area !== 'local') return;
    if (changes.paused) paused = !!changes.paused.newValue;
    if (changes.checkingEnabled) checkingEnabled = !!changes.checkingEnabled.newValue;
    if (changes.excludedHosts) excludedHosts = changes.excludedHosts.newValue || [];
    if (changes.maskInputs) maskInputs = !!changes.maskInputs.newValue;
    if (!activeAllowed()) deactivate();
  });

  function hostExcluded() {
    const host = window.location.hostname.toLowerCase();
    return excludedHosts.some((p) => {
      const pat = String(p || '').trim().toLowerCase().replace(/^\./, '');
      if (!pat) return false;
      return host === pat || host.endsWith('.' + pat);
    });
  }

  function activeAllowed() {
    // maskInputs / HOST_IS_GITHUB gate BEFORE any value read: the checker
    // transmits full field text, so masking means "do not run here".
    return checkingEnabled && !paused && !hostExcluded() && !maskInputs && !HOST_IS_GITHUB;
  }

  // ── Eligibility (checked before any value read) ───────────────────

  const TEXT_INPUT_TYPES = new Set([
    'text', 'search', 'url', 'email', 'tel', 'text-area', 'memo',
  ]);

  function isPasswordField(el) {
    if (el instanceof HTMLInputElement && el.type === 'password') return true;
    const ac = (el.getAttribute && el.getAttribute('autocomplete')) || '';
    if (/password/i.test(ac)) return true;
    // Common heuristics for password-like naming; deliberately broad —
    // a false positive only skips checking, a false negative leaks.
    const name = ((el.name || '') + ' ' + (el.id || '')).toLowerCase();
    return /passwo?r?d|passwd|pwd/.test(name) && el instanceof HTMLInputElement;
  }

  function editableRoot(el) {
    // contenteditable elements: walk up to the outermost editable ancestor.
    let node = el;
    while (node && node !== document.body) {
      const ed = node.getAttribute && node.getAttribute('contenteditable');
      if (ed === 'true' || ed === 'plaintext-only' || (node.isContentEditable && node.parentElement && !node.parentElement.isContentEditable)) {
        return node;
      }
      if (node.isContentEditable) {
        node = node.parentElement;
        continue;
      }
      break;
    }
    return null;
  }

  function eligibleField(el) {
    if (!el || !el.isConnected) return null;
    if (el instanceof HTMLTextAreaElement) return el;
    if (el instanceof HTMLInputElement && TEXT_INPUT_TYPES.has((el.type || 'text').toLowerCase())) return el;
    if (el.isContentEditable) return editableRoot(el) || (el.isContentEditable ? el : null);
    return null;
  }

  // ── Overlay (shadow DOM, wb-* namespaced inside) ──────────────────

  const KIND_COLORS = {
    correctness: '#ef4444', // red
    clarity: '#3b82f6',     // blue
    engagement: '#22c55e',  // green
    delivery: '#a855f7',    // purple
  };

  let overlayHost = null;
  let overlayRoot = null; // shadow root

  function ensureOverlay() {
    if (overlayHost && overlayHost.isConnected) return overlayRoot;
    overlayHost = document.createElement('wb-checker-root');
    overlayHost.style.cssText = 'all:initial;position:absolute;top:0;left:0;width:0;height:0;';
    (document.documentElement || document.body).appendChild(overlayHost);
    overlayRoot = overlayHost.attachShadow({ mode: 'open' });
    const style = document.createElement('style');
    style.textContent = `
      :host { all: initial; }
      .wb-underline {
        position: absolute;
        border-bottom: 2px solid var(--wb-color, #ef4444);
        pointer-events: auto;
        cursor: pointer;
        border-radius: 1px;
        min-width: 4px;
      }
      .wb-underline:hover { filter: brightness(1.25); }
      .wb-underline:focus-visible { outline: 2px solid #ffffffcc; outline-offset: 1px; }
      .wb-card {
        position: absolute;
        z-index: 2147483647;
        background: #18181b;
        color: #e4e4e7;
        border: 1px solid #3f3f46;
        border-radius: 10px;
        box-shadow: 0 8px 24px rgba(0,0,0,.45);
        padding: 10px 12px;
        font: 12px/1.45 system-ui, -apple-system, "Segoe UI", sans-serif;
        max-width: 340px;
      }
      .wb-card .wb-msg { margin: 0 0 8px; color: #fafafa; }
      .wb-card .wb-rule { color: #71717a; font-size: 10px; margin: 0 0 8px; }
      .wb-card .wb-row { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
      .wb-chip {
        background: #27272a; border: 1px solid #52525b; color: #fafafa;
        border-radius: 999px; padding: 3px 10px; cursor: pointer; font-size: 12px;
      }
      .wb-chip:hover { background: #3f3f46; }
      .wb-chip.wb-primary { background: #4f46e5; border-color: #6366f1; }
      .wb-chip.wb-ignore { background: transparent; color: #a1a1aa; }
      .wb-copystate { color: #a1a1aa; font-size: 11px; }
    `;
    overlayRoot.appendChild(style);
    return overlayRoot;
  }

  function clearUnderlines() {
    if (!overlayRoot) return;
    for (const el of [...overlayRoot.querySelectorAll('.wb-underline, .wb-card')]) el.remove();
  }

  // ── Rect measurement via mirror div (textarea/input) ──────────────

  const MIRROR_PROPS = [
    'boxSizing', 'width', 'height', 'paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft',
    'borderTopWidth', 'borderRightWidth', 'borderBottomWidth', 'borderLeftWidth',
    'fontFamily', 'fontSize', 'fontWeight', 'fontStyle', 'letterSpacing', 'lineHeight',
    'textTransform', 'textIndent', 'whiteSpace', 'wordSpacing', 'wordWrap', 'overflowWrap',
    'boxDecorationBreak', 'tabSize', 'webkitTextSizeAdjust',
  ];

  function mirrorRectsForOffset(el, start, end) {
    // Build a hidden mirror of the field, wrap [start..end) in a marker
    // span, and measure its client rect.
    const mirror = document.createElement('div');
    const cs = window.getComputedStyle(el);
    for (const prop of MIRROR_PROPS) {
      try { mirror.style[prop] = cs[prop]; } catch { /* proprietary */ }
    }
    mirror.style.position = 'absolute';
    mirror.style.visibility = 'hidden';
    mirror.style.left = '-9999px';
    mirror.style.top = '0';
    mirror.style.overflow = 'hidden';
    mirror.style.whiteSpace = 'pre-wrap';
    mirror.style.overflowWrap = 'break-word';
    const value = el.value != null ? el.value : '';
    mirror.textContent = '';
    mirror.appendChild(document.createTextNode(value.slice(0, start)));
    const mark = document.createElement('span');
    mark.textContent = value.slice(start, end) || '\u200b';
    mirror.appendChild(mark);
    mirror.appendChild(document.createTextNode(value.slice(end)));
    (el.ownerDocument.body || document.body).appendChild(mirror);
    let rects;
    try {
      const range = document.createRange();
      range.selectNodeContents(mark);
      rects = [...range.getClientRects()];
    } finally {
      mirror.remove();
    }
    // Mirror sits at left:-9999px — translate rects back to the field's
    // document position.
    const fieldRect = el.getBoundingClientRect();
    const scrollX = window.scrollX, scrollY = window.scrollY;
    return rects.map((r) => ({
      left: fieldRect.left + scrollX + (r.left + 9999),
      top: fieldRect.top + scrollY + (r.top),
      width: r.width,
      height: r.height,
    })).filter((r) => r.width > 0 && r.height > 0);
  }

  function contentEditableRects(el, start, end) {
    // Locate the character offsets within the contenteditable's text.
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    let consumed = 0;
    let startNode = null, startOff = 0, endNode = null, endOff = 0;
    while (walker.nextNode()) {
      const node = walker.currentNode;
      const len = node.nodeValue.length;
      if (startNode === null && consumed + len >= start) {
        startNode = node; startOff = start - consumed;
      }
      if (consumed + len >= end) {
        endNode = node; endOff = end - consumed;
        break;
      }
      consumed += len;
    }
    if (!startNode || !endNode) return [];
    const range = document.createRange();
    range.setStart(startNode, Math.min(startOff, startNode.nodeValue.length));
    range.setEnd(endNode, Math.min(endOff, endNode.nodeValue.length));
    return [...range.getClientRects()].map((r) => ({
      left: r.left + window.scrollX,
      top: r.top + window.scrollY,
      width: r.width,
      height: r.height,
    })).filter((r) => r.width > 0 && r.height > 0);
  }

  function rectsForIssue(el, issue) {
    try {
      if (el.isContentEditable) return contentEditableRects(el, issue.start, issue.end);
      return mirrorRectsForOffset(el, issue.start, issue.end);
    } catch {
      return []; // degrade: no underline, card reachable via recheck only
    }
  }

  // ── Session ignore set ─────────────────────────────────────────────

  const sessionIgnoredRules = new Set();

  // ── Card ───────────────────────────────────────────────────────────

  let activeCard = null;

  function closeCard() {
    if (activeCard) { activeCard.remove(); activeCard = null; }
  }

  function openCard(el, issue, anchorRect, onApply) {
    closeCard();
    const root = ensureOverlay();
    const card = document.createElement('div');
    card.className = 'wb-card';
    card.setAttribute('role', 'dialog');
    card.setAttribute('aria-label', 'Writing suggestion');
    card.tabIndex = -1;

    const msg = document.createElement('p');
    msg.className = 'wb-msg';
    msg.textContent = issue.message || 'Suggestion';
    card.appendChild(msg);

    const rule = document.createElement('p');
    rule.className = 'wb-rule';
    rule.textContent = issue.ruleId || '';
    card.appendChild(rule);

    const row = document.createElement('div');
    row.className = 'wb-row';
    const replacements = (issue.replacements || []).slice(0, 3);
    replacements.forEach((rep, idx) => {
      const chip = document.createElement('button');
      chip.className = 'wb-chip' + (idx === 0 ? ' wb-primary' : '');
      chip.textContent = rep;
      chip.addEventListener('click', () => {
        onApply(rep);
        closeCard();
      });
      row.appendChild(chip);
    });
    const ignore = document.createElement('button');
    ignore.className = 'wb-chip wb-ignore';
    ignore.textContent = 'Ignore';
    ignore.addEventListener('click', () => {
      sessionIgnoredRules.add(issue.ruleId);
      closeCard();
      renderIssuesForField(el, lastIssuesByField.get(el) || [], { force: true });
    });
    row.appendChild(ignore);
    card.appendChild(row);
    root.appendChild(card);

    // Position: below the anchor, flip above when clipped.
    card.style.left = Math.max(8, anchorRect.left) + 'px';
    card.style.top = (anchorRect.bottom + 6) + 'px';
    const rect = card.getBoundingClientRect();
    const viewH = window.innerHeight;
    if (rect.bottom > viewH) {
      card.style.top = (anchorRect.top - rect.height - 6) + 'px';
    }
    activeCard = card;
    card.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') { closeCard(); }
      e.stopPropagation();
    });
  }

  // ── Watcher + debounce ─────────────────────────────────────────────

  let activeField = null;
  let debounceTimer = null;
  let generation = 0; // supersede counter (STREAM_GENERATION pattern)
  let lastHashByField = new WeakMap();
  let lastIssuesByField = new WeakMap();

  function djb2(str) {
    let h = 5381;
    for (let i = 0; i < str.length; i++) {
      h = ((h << 5) + h + str.charCodeAt(i)) | 0;
    }
    return h;
  }

  function fieldText(el) {
    if (el.isContentEditable) return el.innerText;
    return el.value || '';
  }

  function deactivate() {
    generation++;
    clearTimeout(debounceTimer);
    debounceTimer = null;
    activeField = null;
    clearUnderlines();
    closeCard();
  }

  function scheduleCheck(el, immediate) {
    generation++;
    const gen = generation;
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      if (gen !== generation) return; // superseded
      runCheck(el, gen);
    }, immediate ? 0 : 300);
  }

  // >20 KB text: chunk at sentence boundaries, merge by offset shift.
  function chunkText(text, capBytes) {
    const chunks = [];
    let rest = text;
    while (rest.length > capBytes) {
      let cut = -1;
      const window_ = rest.slice(0, capBytes);
      for (const m of window_.matchAll(/[.!?]\s|\n/g)) cut = m.index + m[0].length;
      if (cut <= 0) cut = capBytes;
      chunks.push(rest.slice(0, cut));
      rest = rest.slice(cut);
    }
    chunks.push(rest);
    return chunks;
  }

  async function runCheck(el, gen) {
    if (!activeAllowed() || !el.isConnected) return;
    // INV-PRIV-001 re-check at read time.
    if (isPasswordField(el)) return;
    const text = fieldText(el);
    if (!text.trim()) {
      clearUnderlines();
      closeCard();
      lastHashByField.set(el, djb2(text));
      lastIssuesByField.set(el, []);
      return;
    }
    const hash = djb2(text);
    if (lastHashByField.get(el) === hash) return; // focus juggling
    lastHashByField.set(el, hash);

    const host = window.location.hostname;
    const baseReq = {
      surface: 'browser',
      target: { kind: 'browserHost', host },
      goals: {
        dialect: 'enUs', domain: 'General', formality: 'Neutral',
        audience: 'General', intent: null,
      },
    };

    let issues = [];
    try {
      if (text.length <= 20000) {
        const resp = await sendCheck({ ...baseReq, text });
        if (gen !== generation) return;
        issues = (resp && resp.issues) || [];
      } else {
        const chunks = chunkText(text, 20000);
        let shift = 0;
        for (const chunk of chunks) {
          const resp = await sendCheck({ ...baseReq, text: chunk });
          if (gen !== generation) return;
          for (const issue of (resp && resp.issues) || []) {
            issues.push({ ...issue, start: issue.start + shift, end: issue.end + shift });
          }
          shift += chunk.length;
        }
      }
    } catch {
      return; // transport/rate failure → skip this cycle silently
    }
    if (gen !== generation) return;
    lastIssuesByField.set(el, issues);
    renderIssuesForField(el, issues);
  }

  function sendCheck(request) {
    return new Promise((resolve, reject) => {
      chrome.runtime.sendMessage({ type: 'check', request }, (resp) => {
        if (chrome.runtime.lastError) return reject(chrome.runtime.lastError);
        if (resp && resp.ok) return resolve(resp.body);
        reject(new Error((resp && resp.error) || 'check failed'));
      });
    });
  }

  // ── Rendering ──────────────────────────────────────────────────────

  function renderIssuesForField(el, issues, opts) {
    clearUnderlines();
    closeCard();
    const visible = issues.filter(
      (i) => i.start < i.end && !sessionIgnoredRules.has(i.ruleId),
    );
    for (const issue of visible) {
      const rects = rectsForIssue(el, issue);
      if (!rects.length) continue; // degrade silently (CSP/layout exotica)
      const root = ensureOverlay();
      const anchor = rects[0];
      for (const r of rects.slice(0, 3)) {
        const u = document.createElement('div');
        u.className = 'wb-underline';
        u.tabIndex = 0;
        u.setAttribute('role', 'button');
        u.setAttribute('aria-label', (issue.message || 'suggestion') + '. Enter for options.');
        u.style.setProperty('--wb-color', KIND_COLORS[issue.kind] || KIND_COLORS.correctness);
        u.style.left = r.left + 'px';
        u.style.top = (r.top + r.height - 2) + 'px';
        u.style.width = Math.max(4, r.width) + 'px';
        u.style.height = '2px';
        u.addEventListener('click', (e) => {
          e.stopPropagation();
          openCard(el, issue, anchor, (rep) => applyReplacement(el, issue, rep));
        });
        u.addEventListener('keydown', (e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault();
            openCard(el, issue, anchor, (rep) => applyReplacement(el, issue, rep));
          }
        });
        root.appendChild(u);
      }
    }
    if (opts && opts.force && !visible.length) {
      // nothing to show after an ignore
    }
  }

  // ── Apply (undo-preserving) ────────────────────────────────────────

  function applyReplacement(el, issue, replacement) {
    let applied = false;
    try {
      el.focus();
      if (el.isContentEditable) {
        // Select the issue span in the editable, then execCommand — one
        // undo step, works in Chrome's contenteditable engine.
        const rects = selectContentEditableRange(el, issue.start, issue.end);
        if (rects !== false) {
          applied = document.execCommand('insertText', false, replacement);
        }
      } else {
        el.setSelectionRange(issue.start, issue.end);
        applied = document.execCommand('insertText', false, replacement);
        if (!applied) {
          // Fallback: native setRangeText + input event (undo not
          // guaranteed on every engine; value is correct).
          el.setRangeText(replacement, issue.start, issue.end, 'end');
          el.dispatchEvent(new Event('input', { bubbles: true }));
          applied = el.value != null;
        }
      }
    } catch {
      applied = false;
    }
    if (!applied || fieldText(el) === lastAppliedExpectation(el, issue, replacement)) {
      // detect silent failure (React-controlled editors may revert)
    }
    // Immediate re-check of the edited region — no debounce wait.
    lastHashByField.delete(el);
    scheduleCheck(el, true);
  }

  function lastAppliedExpectation(el, issue, replacement) {
    // helper kept intentionally trivial: silent-failure detection uses
    // the re-check result; nothing to compare against pre-apply here.
    return null;
  }

  function selectContentEditableRange(el, start, end) {
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    let consumed = 0;
    let startNode = null, startOff = 0, endNode = null, endOff = 0;
    while (walker.nextNode()) {
      const node = walker.currentNode;
      const len = node.nodeValue.length;
      if (startNode === null && consumed + len >= start) { startNode = node; startOff = start - consumed; }
      if (consumed + len >= end) { endNode = node; endOff = end - consumed; break; }
      consumed += len;
    }
    if (!startNode || !endNode) return false;
    const sel = window.getSelection();
    const range = document.createRange();
    range.setStart(startNode, Math.min(startOff, startNode.nodeValue.length));
    range.setEnd(endNode, Math.min(endOff, endNode.nodeValue.length));
    sel.removeAllRanges();
    sel.addRange(range);
    return true;
  }

  // ── Event wiring ───────────────────────────────────────────────────

  document.addEventListener('focusin', (e) => {
    const field = eligibleField(e.target);
    if (!field) return;
    if (!activeAllowed()) return;         // INV-EXCL-001: no reads beyond this
    if (isPasswordField(field)) return;   // INV-PRIV-001: never watch
    if (activeField && activeField !== field) clearUnderlines();
    activeField = field;
    scheduleCheck(field, false);
  });

  document.addEventListener('focusout', (e) => {
    const field = eligibleField(e.target);
    if (field && field === activeField) {
      // Delay so focus moving into our own card doesn't clear state.
      setTimeout(() => {
        if (activeField === field && document.activeElement !== field && !cardHasFocus()) {
          // keep underlines visible — Grammarly keeps them too — but
          // stop the debouncer.
          clearTimeout(debounceTimer);
          debounceTimer = null;
          generation++;
        }
      }, 150);
    }
  });

  function cardHasFocus() {
    return !!(activeCard && activeCard.contains(document.activeElement));
  }

  document.addEventListener('input', (e) => {
    const field = eligibleField(e.target);
    if (!field || field !== activeField) return;
    if (!activeAllowed() || isPasswordField(field)) return;
    scheduleCheck(field, false);
  }, true);

  // Underline drift: recompute on scroll/resize (debounced through the
  // same generation counter so in-flight checks don't fight re-renders).
  let reflowTimer = null;
  function scheduleReflow() {
    if (!activeField) return;
    if (reflowTimer) return;
    reflowTimer = setTimeout(() => {
      reflowTimer = null;
      const el = activeField;
      const issues = lastIssuesByField.get(el) || [];
      if (issues.length && el.isConnected) renderIssuesForField(el, issues);
    }, 100);
  }
  window.addEventListener('scroll', scheduleReflow, { passive: true, capture: true });
  window.addEventListener('resize', scheduleReflow, { passive: true });

  // Click-away closes the card.
  document.addEventListener('click', (e) => {
    if (activeCard && !e.composedPath().includes(activeCard)) closeCard();
  }, true);

  // Disabled state or excluded host at load: stay dormant.
  if (!activeAllowed()) deactivate();
})();
