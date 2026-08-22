// popup.js — Extension popup UI controller

const dot         = document.getElementById('dot');
const statusEl    = document.getElementById('status-text');
const infoEl      = document.getElementById('info');
const tokenIn     = document.getElementById('token');
const tokenShow   = document.getElementById('token-show');
const portIn      = document.getElementById('port');
const pauseIn     = document.getElementById('pause');
const maskIn      = document.getElementById('mask-inputs');
const saveBtn     = document.getElementById('save');
const copyBtn     = document.getElementById('copy');
const toast       = document.getElementById('toast');

// Ports WordBuddy tries, in order. Must match background.js + Rust.
const WORDBUDDY_PORTS = [19521, 19522, 19523];

// ── Load saved config ──────────────────────────────────────────────

chrome.storage.local.get(
  ['token', 'port', 'paused', 'maskInputs'],
  (cfg) => {
    tokenIn.value  = cfg.token || '';
    portIn.value   = cfg.port  || WORDBUDDY_PORTS[0];
    pauseIn.checked = !!cfg.paused;
    maskIn.checked  = !!cfg.maskInputs;
    checkStatus();
  },
);

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
    setStatus(false, 'No token configured');
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
        setStatus(true, `Connected to WordBuddy (port ${port})`);
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

saveBtn.addEventListener('click', () => {
  const token = tokenIn.value.trim();
  const rawPort = parseInt(portIn.value, 10);
  // Only allow the three ports the Rust server binds to — any other
  // value would silently fail to connect.
  const port = WORDBUDDY_PORTS.includes(rawPort) ? rawPort : WORDBUDDY_PORTS[0];
  portIn.value = port;

  chrome.storage.local.set(
    {
      token,
      port,
      paused: pauseIn.checked,
      maskInputs: maskIn.checked,
    },
    () => {
      flash('Saved');
      checkStatus();
    },
  );
});

// ── Copy ───────────────────────────────────────────────────────────

copyBtn.addEventListener('click', () => {
  navigator.clipboard.writeText(tokenIn.value).then(() => {
    copyBtn.textContent = 'Copied!';
    setTimeout(() => { copyBtn.textContent = 'Copy Token'; }, 1500);
  });
});

// ── Show / hide token ──────────────────────────────────────────────

tokenShow.addEventListener('click', () => {
  const showing = tokenIn.type === 'text';
  tokenIn.type = showing ? 'password' : 'text';
  tokenShow.textContent = showing ? 'show' : 'hide';
});

// ── Helpers ────────────────────────────────────────────────────────

function flash(msg) {
  toast.textContent   = msg;
  toast.style.display = 'block';
  setTimeout(() => { toast.style.display = 'none'; }, 2000);
}
