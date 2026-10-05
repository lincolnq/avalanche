#!/usr/bin/env node
// Screenshot the Desktop UI in headless Chrome against the mock service — no
// Tauri window, server, or unlocked screen needed (see src/dev/browserPreview.ts
// and desktop/CLAUDE.md "Debugging the running app").
//
//   node scripts/preview-shot.mjs out.png [--dark] [--size 1200x800]
//        [--query 'accounts=2'] [--eval '<js function body>']...
//
// Needs the Vite dev server on http://localhost:1420 (`make desktop` or
// `npm run dev`). `--eval` snippets run in order before the screenshot and may
// use the window.__av helpers, e.g. --eval "await __av.open('General')".
import { spawn } from "node:child_process";
import { writeFileSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const BASE_URL = process.env.AV_PREVIEW_URL ?? "http://localhost:1420/";

const args = process.argv.slice(2);
let out = null;
let dark = false;
let size = "1200x800";
let query = "";
const evals = [];
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (a === "--dark") dark = true;
  else if (a === "--size") size = args[++i];
  else if (a === "--eval") evals.push(args[++i]);
  else if (a === "--query") query = args[++i];
  else out = a;
}
if (!out) {
  console.error("usage: preview-shot.mjs out.png [--dark] [--size WxH] [--eval js]...");
  process.exit(2);
}
const [width, height] = size.split("x").map(Number);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const port = 9300 + Math.floor(Math.random() * 500);
const chrome = spawn(
  CHROME,
  [
    "--headless=new",
    `--remote-debugging-port=${port}`,
    `--user-data-dir=${mkdtempSync(join(tmpdir(), "av-preview-"))}`,
    "--no-first-run",
    "--hide-scrollbars",
    "about:blank",
  ],
  { stdio: "ignore" },
);

async function main() {
  let targets;
  for (let i = 0; i < 50; i++) {
    try {
      targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      if (targets.some((t) => t.type === "page")) break;
    } catch {}
    await sleep(100);
  }
  const page = targets.find((t) => t.type === "page");
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((r, j) => { ws.onopen = r; ws.onerror = j; });
  let nextId = 1;
  const pending = new Map();
  ws.onmessage = (ev) => {
    const msg = JSON.parse(ev.data);
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result);
    }
  };
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = nextId++;
      pending.set(id, { resolve, reject });
      ws.send(JSON.stringify({ id, method, params }));
    });
  const evaluate = async (body) => {
    const r = await send("Runtime.evaluate", {
      expression: `(async () => { ${body} })()`,
      awaitPromise: true,
      returnByValue: true,
    });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
    return r.result.value;
  };

  await send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 2, mobile: false });
  await send("Emulation.setEmulatedMedia", {
    features: [{ name: "prefers-color-scheme", value: dark ? "dark" : "light" }],
  });
  await send("Page.enable");
  await send("Page.navigate", { url: query ? `${BASE_URL}?${query}` : BASE_URL });
  // Wait for the app to restore the preview account and render Chats.
  for (let i = 0; i < 100; i++) {
    const ready = await evaluate("return !!(window.__av && __av.state().conversations.length)").catch(() => false);
    if (ready) break;
    await sleep(100);
  }
  await evaluate(
    "const s = new CSSStyleSheet(); s.replaceSync('*{transition:none !important; animation:none !important}');" +
      "document.adoptedStyleSheets = [...document.adoptedStyleSheets, s];",
  );
  for (const e of evals) {
    const v = await evaluate(e);
    if (v !== undefined && v !== null) console.log(typeof v === "string" ? v : JSON.stringify(v));
    await sleep(250);
  }
  await sleep(300);
  const shot = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(out, Buffer.from(shot.data, "base64"));
  console.log(out);
  ws.close();
}

main()
  .catch((e) => {
    console.error(e.message ?? e);
    process.exitCode = 1;
  })
  .finally(() => chrome.kill());
