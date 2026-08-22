// CDP driver for the WordBuddy widget window (PLAN-04 gate).
// Usage: node cdp-widget.mjs <expression-file> [--target widget|main]
// Evaluates the expression file's contents inside the chosen target and
// prints the JSON result.

const list = await fetch("http://127.0.0.1:9223/json/list").then((r) => r.json());
const wantWidget = process.argv[3] !== "main";
const candidates = list.filter(
  (t) => t.type === "page",
);
// The widget window title is "WordBuddy suggestions"; main is "WordBuddy".
// Titles are identical (same SPA); probe each page for its window label.
async function labelOf(t) {
  const ws = new WebSocket(t.webSocketDebuggerUrl);
  await new Promise((r, j) => { ws.onopen = r; ws.onerror = j; });
  const val = await new Promise((resolve) => {
    ws.onmessage = (ev) => {
      const msg = JSON.parse(ev.data);
      if (msg.id === 1) {
        resolve(msg.result?.result?.value ?? "?");
      }
    };
    ws.send(JSON.stringify({
      id: 1,
      method: "Runtime.evaluate",
      params: { expression: "window.__wbLabel ?? 'unknown'", returnByValue: true },
    }));
  });
  ws.close();
  return val;
}
let target = null;
for (const t of candidates) {
  const label = await labelOf(t);
  if (wantWidget && label === "widget") { target = t; break; }
  if (!wantWidget && label !== "widget") { target = t; break; }
}
if (!target) {
  console.log(JSON.stringify({ error: "target not found" }));
  process.exit(1);
}

const expr = (await import("node:fs")).readFileSync(process.argv[2], "utf8");

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
ws.onerror = (e) => {
  console.log(JSON.stringify({ error: "ws error" }));
  process.exit(1);
};
await new Promise((r) => (ws.onopen = r));
await send("Runtime.enable");
const result = await send("Runtime.evaluate", {
  expression: `(async () => { ${expr} })()`,
  awaitPromise: true,
  returnByValue: true,
});
if (result.exceptionDetails) {
  console.log(JSON.stringify({ exception: result.exceptionDetails }));
} else {
  console.log(JSON.stringify(result.result.value));
}
ws.close();
process.exit(0);
