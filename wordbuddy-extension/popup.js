// popup.js — Extension popup UI controller

const dot         = document.getElementById('dot');
const statusEl    = document.getElementById('status-text');
const infoEl      = document.getElementById('info');
const tokenIn     = document.getElementById('token');
const tokenShow   = document.getElementById('token-show');
const portIn      = document.getElementById('port');
const pauseIn     = document.getElementById('pause');
const maskIn      = document.getElementById('mask-inputs');
const styleIn     = document.getElementById('style-site');
const styleInfo   = document.getElementById('style-site-info');
const mutedRow    = document.getElementById('muted-row');
const mutedCount  = document.getElementById('muted-count');
const unmuteBtn   = document.getElementById('unmute');
const siteRow     = document.getElementById('site-row');
const siteInfo    = document.getElementById('site-info');
const enableBtn   = document.getElementById('enable-site');
const saveBtn     = document.getElementById('save');
const copyBtn     = document.getElementById('copy');
const toast       = document.getElementById('toast');

// activeTab is granted only while the user has opened this popup. It lets the
// user explicitly enable WordBuddy for the current site without a permanent
// blanket "tabs" permission or a pre-existing content script.
async function activePage() {
  try {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    if (!tab?.id || !tab.url) return null;
    const url = new URL(tab.url);
    if (!/^https?:$/.test(url.protocol) || !url.hostname) return null;
    return { tab, host: url.hostname.toLowerCase() };
  } catch {
    return null;
  }
}

function showMutedRules(value) {
  const count = Array.isArray(value) ? value.length : 0;
  mutedCount.textContent = String(count);
  mutedRow.style.display = count ? 'flex' : 'none';
}

// Ports WordBuddy tries, in order. Must match background.js + Rust.
const WORDBUDDY_PORTS = [19521, 19522, 19523];

// ── Load saved config ──────────────────────────────────────────────

chrome.storage.local.get(
  ['token', 'port', 'paused', 'maskInputs', 'styleSites', 'ignoredRules'],
  async (cfg) => {
    tokenIn.value  = cfg.token || '';
    portIn.value   = cfg.port  || WORDBUDDY_PORTS[0];
    pauseIn.checked = !!cfg.paused;
    maskIn.checked  = !!cfg.maskInputs;

    const sites = Array.isArray(cfg.styleSites) ? cfg.styleSites.map((s) => String(s).toLowerCase()) : [];
    showMutedRules(cfg.ignoredRules);
    const page = await activePage();
    if (page) {
      const { host, tab } = page;
      const inlineCheckingExcluded = host === 'github.com' || host.endsWith('.github.com');
      styleIn.checked = !inlineCheckingExcluded && sites.includes(host);
      styleIn.disabled = inlineCheckingExcluded;
      styleInfo.textContent = inlineCheckingExcluded
        ? 'Inline checking is disabled on github.com to protect draft and repository content.'
        : `Correctness checks run everywhere; clarity/engagement/delivery need this opt-in. (${host})`;

      // Site coverage: static manifest matches always run; anything
      // else needs a granted optional origin ("Enable" button).
      const STATIC_MATCHES = ['limitless.exchange', 'github.com'];
      const covered =
        STATIC_MATCHES.some((m) => host === m || host.endsWith('.' + m)) ||
        (await new Promise((r) =>
          chrome.permissions.contains(
            { origins: [`*://${host}/*`] },
            (ok) => r(!chrome.runtime.lastError && ok),
          ),
        ));
      if (!covered) {
        siteRow.style.display = 'flex';
        siteRow.dataset.origin = `*://${host}/*`;
        siteInfo.style.display = 'block';
        siteInfo.textContent = `${host} is not in WordBuddy's site list yet.`;
        enableBtn.onclick = () => {
          chrome.permissions.request(
            { origins: [siteRow.dataset.origin] },
            (granted) => {
              if (granted) {
                flash(`Enabled on ${host}`);
                if (tab?.id) chrome.tabs.reload(tab.id);
                siteRow.style.display = 'none';
                siteInfo.style.display = 'none';
              } else {
                flash('Permission was not granted');
              }
            },
          );
        };
      } else {
        siteRow.style.display = 'none';
        siteInfo.style.display = 'none';
      }
    } else {
      styleIn.checked = false;
      styleIn.disabled = true;
      styleInfo.textContent = 'Open a page WordBuddy connects to, to change this site.';
      siteRow.style.display = 'none';
      siteInfo.style.display = 'none';
    }
    checkStatus();
  },
);

unmuteBtn.addEventListener('click', () => {
  chrome.storage.local.set({ ignoredRules: [] }, () => {
    showMutedRules([]);
    flash('Unmuted all rules');
  });
});

chrome.storage.onChanged.addListener((changes, area) => {
  if (area === 'local' && changes.ignoredRules) showMutedRules(changes.ignoredRules.newValue);
});

// ── Status check ───────────────────────────────────────────────────

async function checkStatus() {
  const cfg = await new Promise(r =>
    chrome.storage.local.get(['token', 'port', 'paused'], r),
  );
  const token = cfg.token;

  if (cfg.paused) {
    setStatus(false, 'Paused — scanning disabled');
    return;
  }

  if (!token) {
    setStatus(false, 'No token — WordBuddy → Settings → Extension token → Copy, paste above');
    return;
  }

  // Try saved port first, then the fallback list.
  const candidates = [...new Set([cfg.port || WORDBUDDY_PORTS[0], ...WORDBUDDY_PORTS])];
  for (const port of candidates) {
    try {
      const resp = await fetch(`http://127.0.0.1:${port}/status`);
      if (resp.ok) {
        // Remember the working port.
        if (port !== cfg.port) {
          chrome.storage.local.set({ port });
          portIn.value = port;
        }
        // /status is unauthenticated — a green dot on it alone would
        // lie about the token. Validate with an authenticated /ping.
        const ping = await fetch(`http://127.0.0.1:${port}/ping`, {
          headers: { 'Authorization': `Bearer ${token}` },
        });
        if (ping.ok) {
          setStatus(true, `Connected (port ${port})`);
        } else {
          setStatus(false, `Token rejected (port ${port}) — copy a fresh one from WordBuddy Settings`);
        }
        return;
      }
    } catch {
      // try next port
    }
  }
  setStatus(false, 'WordBuddy not running');
}

function setStatus(ok, text) {
  dot.className       = ok ? 'dot on' : 'dot off';
  statusEl.textContent = text;
  statusEl.style.color = ok ? '#10b981' : '#ef4444';
}

// ── Save ───────────────────────────────────────────────────────────

saveBtn.addEventListener('click', async () => {
  const token = tokenIn.value.trim();
  const rawPort = parseInt(portIn.value, 10);
  // Only allow the three ports the Rust server binds to — any other
  // value would silently fail to connect.
  const port = WORDBUDDY_PORTS.includes(rawPort) ? rawPort : WORDBUDDY_PORTS[0];
  portIn.value = port;

  const updates = {
    token,
    port,
    paused: pauseIn.checked,
    maskInputs: maskIn.checked,
  };

  // Per-site style allowlist — only mutate when we know the host,
  // so a Save from an unrelated page can't corrupt other sites' opt-ins.
  if (!styleIn.disabled) {
    const page = await activePage();
    if (page) {
      const { host } = page;
      const cfg = await new Promise((r) =>
        chrome.storage.local.get(['styleSites'], r),
      );
      const sites = new Set(
        (Array.isArray(cfg.styleSites) ? cfg.styleSites : []).map((s) => String(s).toLowerCase()),
      );
      if (styleIn.checked) sites.add(host);
      else sites.delete(host);
      updates.styleSites = [...sites];
    }
  }

  chrome.storage.local.set(updates, () => {
    flash('Saved');
    checkStatus();
  });
});

// ── Copy ───────────────────────────────────────────────────────────

copyBtn.addEventListener('click', () => {
  navigator.clipboard.writeText(tokenIn.value).then(() => {
    copyBtn.textContent = 'Copied!';
    setTimeout(() => { copyBtn.textContent = 'Copy Token'; }, 1500);
  }).catch(() => flash('Could not copy token'));
});

// ── Show / hide token ──────────────────────────────────────────────

tokenShow.addEventListener('click', () => {
  const showing = tokenIn.type === 'text';
  tokenIn.type = showing ? 'password' : 'text';
  tokenShow.textContent = showing ? 'show' : 'hide';
  tokenShow.setAttribute('aria-label', showing ? 'Show token' : 'Hide token');
});

// ── Helpers ────────────────────────────────────────────────────────

function flash(msg) {
  toast.textContent   = msg;
  toast.style.display = 'block';
  clearTimeout(flash.timer);
  flash.timer = setTimeout(() => { toast.style.display = 'none'; }, 2000);
}
