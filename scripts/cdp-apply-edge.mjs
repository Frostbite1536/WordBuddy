// Apply drill driver for the EDGE playground (CDP 9333).
// Usage: node cdp-apply-edge.mjs
const list = await fetch("http://127.0.0.1:9333/json/list").then((r) => r.json());
const target = list.find((t) => t.type === "page" && t.url.includes("playground"));
if (!target) { console.log(JSON.stringify({ error: "no playground target" })); process.exit(1); }
const ws = new WebSocket(target.webSocketDebuggerUrl);
let id = 0;
const pending = new Map();
function send(method, params) {
  return new Promise((resolve, reject) => {
    const msgId = ++id;
    pending.set(msgId, { resolve, reject });
    ws.send(JSON.stringify({ id: msgId, method, params }));
  });
}
ws.onmessage = (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id && pending.has(msg.id)) {
    const p = pending.get(msg.id);
    pending.delete(msg.id);
    if (msg.error) p.reject(new Error(msg.error.message));
    else p.resolve(msg.result);
  }
};
await new Promise((r) => (ws.onopen = r));

async function evalJs(expr) {
  const res = await send("Runtime.evaluate", {
    expression: `(async () => { ${expr} })()`,
    awaitPromise: true,
    returnByValue: true,
    userGesture: true,
  });
  return res.exceptionDetails ? { exception: res.exceptionDetails.text } : res.result.value;
}

// Type seeded errors into the textarea (focus it so the monitor sees the field).
const typed = await evalJs(`
  const el = document.querySelector('#wb-textarea');
  el.focus();
  el.value = 'This is teh smae recieve with a mispeling';
  el.dispatchEvent(new Event('input', { bubbles: true }));
  document.title;
`);
console.log(JSON.stringify({ typed }));
process.exit(0);
