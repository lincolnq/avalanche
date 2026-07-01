// actnet testbot - the first demo Project on the platform.
//
// A standalone HTTP service that serves a tiny web UI and spins up AI chatbot
// accounts on demand. Each bot is a full Signal-protocol participant: it
// registers on the homeserver, holds its own identity keys, and sends/receives
// encrypted DMs via `@actnet/app-core`. Bots converse using Claude Haiku; an
// Anthropic key is required (configured via `.env` - see `loadDotenv`).
//
// Lifecycle / state:
//   - All bot state is in-memory: the `botsByUser` registry dies when the
//     service restarts. Each bot's SQLCipher store is a throwaway file under
//     the OS temp dir (app-core has no in-memory store binding for node), so
//     restarting the service abandons every bot identity - matching the
//     original Rust testbot's ephemeral semantics.
//   - Each bot runs one `for await` event loop over its own AppCore. Node is
//     single-threaded, so a bot processes its messages sequentially with no
//     locking; libsignal's non-Send constraint that forced dedicated threads
//     in the Rust port simply doesn't apply here (the native module owns its
//     own runtime behind the napi boundary).

import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

import { AppCore, initLogging, type SendTarget } from "@actnet/app-core";

const OPENING = "Hey! I'm a testbot. Ask me anything.";
const HAIKU_MODEL = "claude-haiku-4-5-20251001";

interface Env {
  homeserverUrl: string;
  anthropicApiKey?: string;
  anthropicAuthToken?: string;
  anthropicBaseUrl: string;
  bindHost: string;
  bindPort: number;
  logLevel: string;
  sharedSecret?: string;
  basePath: string;
}

/**
 * Load the repo-root `.env` into `process.env` before reading config, so the
 * bot picks up its `ANTHROPIC_*` credentials regardless of how it's launched:
 * `npm start` (cwd = package dir), `dev.py`, or directly from the repo root.
 *
 * Walks up from this module looking for the nearest `.env`. Uses Node's
 * built-in parser (`process.loadEnvFile`), which strips the `export ` prefix
 * the repo's `.env` uses and does NOT override variables already present in the
 * real environment - so an explicit shell/systemd value still wins, matching
 * dev.py's `setdefault` rule. A missing `.env` is non-fatal (the vars may be
 * supplied directly by the environment, e.g. in production).
 */
function loadDotenv(): void {
  let dir = dirname(fileURLToPath(import.meta.url));
  for (;;) {
    const candidate = join(dir, ".env");
    try {
      process.loadEnvFile(candidate);
      console.log(`testbot: loaded env from ${candidate}`);
      return;
    } catch {
      const parent = dirname(dir);
      if (parent === dir) return; // reached filesystem root, no .env found
      dir = parent;
    }
  }
}

function readEnv(): Env {
  // `TESTBOT_BIND_ADDR` is a `host:port` string (e.g. "0.0.0.0:3001"), matching
  // what dev.py hands every Project service.
  const bindAddr = process.env.TESTBOT_BIND_ADDR ?? "0.0.0.0:3001";
  const lastColon = bindAddr.lastIndexOf(":");
  const bindHost = lastColon === -1 ? "0.0.0.0" : bindAddr.slice(0, lastColon);
  const bindPort = Number.parseInt(lastColon === -1 ? bindAddr : bindAddr.slice(lastColon + 1), 10);
  if (!Number.isInteger(bindPort)) {
    throw new Error(`invalid TESTBOT_BIND_ADDR: ${bindAddr}`);
  }
  // Path prefix this Project is served under (e.g. "/p/testbot/" behind Caddy).
  // Used only to render <base href> so the page's relative URLs resolve under
  // the prefix. Defaults to "/" (dev hits the port directly, no prefix).
  let basePath = process.env.TESTBOT_BASE_PATH ?? "/";
  if (!basePath.startsWith("/")) basePath = `/${basePath}`;
  if (!basePath.endsWith("/")) basePath = `${basePath}/`;
  return {
    homeserverUrl: process.env.HOMESERVER_URL ?? "http://localhost:3000",
    anthropicApiKey: process.env.ANTHROPIC_API_KEY || undefined,
    anthropicAuthToken: process.env.ANTHROPIC_AUTH_TOKEN || undefined,
    anthropicBaseUrl: process.env.ANTHROPIC_BASE_URL ?? "https://api.anthropic.com",
    bindHost,
    bindPort,
    basePath,
    logLevel: process.env.TESTBOT_LOG ?? "info",
    // Bootstrap secret for closed-registration servers (docs/24). Unset on an
    // open server, where it isn't needed.
    sharedSecret: process.env.REGISTRATION_SHARED_SECRET || undefined,
  };
}

// ── Bot registry ─────────────────────────────────────────────────────────────

interface BotInfo {
  did: string;
  deviceId: number;
}

/** user DID → bots that user has spawned. Metadata only, for `/api/bots`. */
const botsByUser = new Map<string, BotInfo[]>();

/** Holds a strong reference to each live AppCore so its background reconnect
 *  task (and the event loop consuming it) isn't garbage-collected while the
 *  service runs. */
const liveBots = new Set<AppCore>();

// ── Auth ───────────────────────────────────────────────────────────────────

/** Thrown by handlers to short-circuit with a specific HTTP status. */
class HttpError extends Error {
  constructor(readonly status: number, message: string) {
    super(message);
  }
}

/**
 * Verify the Project token from the `Authorization: Bearer <token>` header
 * against the homeserver and return the caller's DID. The homeserver is the
 * sole authority on token validity; we just relay the answer.
 */
async function verifyToken(env: Env, authHeader: string | undefined): Promise<string> {
  const token = authHeader?.startsWith("Bearer ") ? authHeader.slice("Bearer ".length) : undefined;
  if (!token) {
    throw new HttpError(401, "missing or malformed Authorization header");
  }

  let resp: Response;
  try {
    resp = await fetch(
      `${env.homeserverUrl}/v1/project-token/verify?token=${encodeURIComponent(token)}`,
    );
  } catch (e) {
    throw new HttpError(502, `homeserver request failed: ${(e as Error).message}`);
  }
  if (!resp.ok) {
    throw new HttpError(401, `homeserver rejected token: ${resp.status}`);
  }

  const body = (await resp.json()) as { did?: string };
  if (!body.did) {
    throw new HttpError(502, "homeserver response missing 'did'");
  }
  console.log(`testbot: verified token for did=${body.did}`);
  return body.did;
}

// ── Web UI ───────────────────────────────────────────────────────────────────

const indexHtml = (basePath: string) => `<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <base href="${basePath}">
    <title>Testbot</title>
    <style>
        body { font-family: -apple-system, system-ui, sans-serif; max-width: 480px; margin: 40px auto; padding: 0 20px; }
        h1 { font-size: 24px; }
        button { font-size: 18px; padding: 12px 24px; cursor: pointer; background: #007AFF; color: white; border: none; border-radius: 8px; }
        button:disabled { background: #999; }
        #status { margin-top: 16px; color: #666; }
    </style>
</head>
<body>
    <h1>Testbot</h1>
    <p>Tap below to start a conversation with an AI chatbot. The bot will send you an encrypted DM.</p>
    <button id="textme" onclick="textMe()">Text Me</button>
    <div id="status"></div>
    <script>
        const params = new URLSearchParams(window.location.search);
        const token = params.get('token');

        async function textMe() {
            const btn = document.getElementById('textme');
            const status = document.getElementById('status');
            btn.disabled = true;
            status.textContent = 'Creating bot...';

            try {
                const resp = await fetch('api/text-me', {
                    method: 'POST',
                    headers: {
                        'Authorization': 'Bearer ' + token,
                        'Content-Type': 'application/json',
                    },
                    body: '{}',
                });
                if (!resp.ok) {
                    status.textContent = 'Error: ' + resp.status;
                    btn.disabled = false;
                    return;
                }
                const data = await resp.json();
                status.textContent = 'Bot created! Opening conversation...';
                window.location.href = 'https://go.theavalanche.net/conversation/' + data.bot.did;
            } catch (e) {
                status.textContent = 'Error: ' + e.message;
                btn.disabled = false;
            }
        }
    </script>
</body>
</html>`;

// ── Bot creation & message loop ────────────────────────────────────────────────

interface ConversationMessage {
  role: "user" | "assistant";
  content: string;
}

/**
 * Spin up a fresh ephemeral bot account, send the opening DM to `userDid`, and
 * launch its background message loop. Returns the bot's public handle.
 */
async function spawnBot(env: Env, userDid: string): Promise<BotInfo> {
  // Throwaway SQLCipher store in a temp dir - the bot identity is meant to die
  // with the process. Empty passphrase is fine for a disposable store.
  const dbPath = join(mkdtempSync(join(tmpdir(), "actnet-testbot-")), "store.db");
  // Present the bootstrap secret (as a plain-member token, no project) so the
  // bot can register against a closed-registration server.
  const inviteToken = env.sharedSecret
    ? AppCore.bootstrapToken(env.homeserverUrl, env.sharedSecret)
    : undefined;
  const core = await AppCore.createBotAccount(env.homeserverUrl, dbPath, "", "Testbot", undefined, inviteToken);
  const botDid = core.did();
  const deviceId = core.deviceId();

  console.log(`testbot: bot ${botDid} created, sending opening DM to ${userDid}`);
  await core.sendDm(userDid, OPENING);

  liveBots.add(core);
  const conversation: ConversationMessage[] = [{ role: "assistant", content: OPENING }];
  // Fire-and-forget: the loop runs for the life of the process. Its promise is
  // kept reachable through `liveBots` (which retains `core`).
  runBotLoop(env, core, botDid, conversation).catch((e) => {
    console.error(`testbot: bot ${botDid} loop crashed: ${(e as Error).message}`);
    liveBots.delete(core);
  });

  return { did: botDid, deviceId };
}

/**
 * The per-bot receive loop. For each inbound DM: pause like a human reading,
 * send a read receipt, react 👍 (a live exercise of the reaction path,
 * docs/33), then generate and send a Claude reply.
 */
async function runBotLoop(
  env: Env,
  core: AppCore,
  botDid: string,
  conversation: ConversationMessage[],
): Promise<void> {
  console.log(`testbot: bot ${botDid} message loop started`);
  for await (const event of core.events()) {
    if (event.kind !== "message") continue;
    const msg = event.message;
    if (msg.senderDid === botDid) continue;

    console.log(`testbot: bot ${botDid} <<< from ${msg.senderDid}: ${JSON.stringify(msg.body)}`);

    // Pause briefly before acknowledging, like a human reading.
    await new Promise((r) => setTimeout(r, 1000));

    const dm: SendTarget = { kind: "dm", recipientDid: msg.senderDid };

    // Read receipt + 👍, both keyed on the message's send-time. Best-effort:
    // a failed acknowledgement shouldn't block the reply.
    if (msg.sentAt) {
      try {
        await core.sendReadReceipt(msg.senderDid, [msg.sentAt]);
      } catch (e) {
        console.error(`testbot: bot ${botDid} read receipt failed: ${(e as Error).message}`);
      }
      try {
        await core.sendReaction(dm, msg.senderDid, msg.sentAt, "👍", false);
      } catch (e) {
        console.error(`testbot: bot ${botDid} reaction failed: ${(e as Error).message}`);
      }
    }

    // The sender's profile_key rides on this very message, so app-core has
    // already fetched + cached their display name by now.
    const displayName = await core.contactDisplayName(msg.senderDid);

    conversation.push({ role: "user", content: msg.body });
    const response = await generateResponse(env, conversation, displayName || undefined);
    conversation.push({ role: "assistant", content: response });

    console.log(`testbot: bot ${botDid} >>> to ${msg.senderDid}: ${JSON.stringify(response)}`);
    try {
      await core.send(dm, response);
    } catch (e) {
      console.error(`testbot: bot ${botDid} reply to ${msg.senderDid} failed: ${(e as Error).message}`);
    }
  }
  console.log(`testbot: bot ${botDid} event stream closed, loop exiting`);
  liveBots.delete(core);
}

// ── Claude API ───────────────────────────────────────────────────────────────

// This bot talks to an Anthropic-compatible `/v1/messages` endpoint. To point
// it at real Claude models (rather than a compatible provider), set these in
// your `.env` (see `loadDotenv` / `.env.example`):
//
//   ANTHROPIC_BASE_URL  The API host. For Claude, use https://api.anthropic.com
//                       (the default if unset). The bot always POSTs to
//                       `${ANTHROPIC_BASE_URL}/v1/messages`. Point this at a
//                       compatible provider (e.g. DeepSeek's Anthropic-compatible
//                       endpoint) only if you're not using first-party Claude.
//   ANTHROPIC_API_KEY   Your Claude API key, created in the Anthropic Console
//                       (console.anthropic.com -> Settings -> API keys). Sent as
//                       the `x-api-key` header below. ANTHROPIC_AUTH_TOKEN is an
//                       alternative bearer credential (e.g. for proxies/gateways).
//   ANTHROPIC_MODEL     The model ID. Use an exact ID, no date suffix. Current
//                       Claude IDs: claude-opus-4-8 (most capable),
//                       claude-sonnet-4-6 (balanced), claude-haiku-4-5 (fastest /
//                       cheapest -- this bot's default tier via HAIKU_MODEL).
//                       ANTHROPIC_DEFAULT_HAIKU_MODEL overrides this if set.

/**
 * Ask Claude Haiku for the next reply. If no API key is configured the bot runs
 * in echo mode (echoes the user's last message) so local dev needs zero setup;
 * on an API error or request failure it likewise falls back to echoing, so the
 * conversation always progresses.
 */
async function generateResponse(
  env: Env,
  conversation: ConversationMessage[],
  userDisplayName: string | undefined,
): Promise<string> {
  const apiKey = env.anthropicApiKey ?? env.anthropicAuthToken;
  // No key configured → echo mode (warned once at startup in `main`). Keeps
  // local dev / `make dev-all` working with zero required configuration.
  if (!apiKey) return echoResponse(conversation);

  const model = process.env.ANTHROPIC_DEFAULT_HAIKU_MODEL
    ?? process.env.ANTHROPIC_MODEL
    ?? HAIKU_MODEL;

  let systemPrompt =
    "You are a friendly chatbot on the Avalanche platform. Keep your responses " +
    "concise and conversational. You're chatting with an activist - be " +
    "supportive and helpful.";
  if (userDisplayName) {
    // Sanitize: strip/replace characters that break JSON or trip strict
    // providers (control chars, en/em dashes, curly quotes) - common in
    // display names set from mobile keyboards.
    const safe = userDisplayName.replace(/[\u0000-\u001f\u007f-\u009f\u2013\u2014\u2018\u2019\u201c\u201d]/g, "'");
    systemPrompt += ` The user's display name is ${safe}.`;
  }

  console.log(`testbot: API call model=${model} url=${env.anthropicBaseUrl}/v1/messages`);
  try {
    const resp = await fetch(`${env.anthropicBaseUrl}/v1/messages`, {
      method: "POST",
      headers: {
        "x-api-key": apiKey,
        "anthropic-version": "2023-06-01",
        "content-type": "application/json",
      },
      body: JSON.stringify({
        model,
        max_tokens: 1024,
        system: systemPrompt,
        messages: conversation.map((m) => ({ role: m.role, content: m.content })),
      }),
    });
    if (!resp.ok) {
      console.error(`testbot: claude API error ${resp.status}: ${await resp.text()}`);
      return echoResponse(conversation);
    }
    const body = (await resp.json()) as { content?: Array<{ type?: string; text?: string; thinking?: string }> };
    // Some models (deepseek-v4-*) return a "thinking" block first; skip
    // those and use the first "text" block.  Fall back to thinking text
    // if no text block is present.
    const textBlock = body.content?.find((b) => b.type === "text");
    const thinkingBlock = body.content?.find((b) => b.type === "thinking");
    const text = textBlock?.text ?? thinkingBlock?.thinking;
    if (!text) {
      console.error("testbot: unexpected API response shape:", JSON.stringify(body).slice(0, 200));
      return "(Unexpected API response - check logs)";
    }
    return text;
  } catch (e) {
    console.error(`testbot: claude API request failed: ${(e as Error).message}`);
    return echoResponse(conversation);
  }
}

/** Fallback when the API errors or is unreachable: echo the user's last message. */
function echoResponse(conversation: ConversationMessage[]): string {
  const lastUser = [...conversation].reverse().find((m) => m.role === "user");
  return `(echo) You said: ${lastUser?.content ?? "..."}`;
}

// ── HTTP server ────────────────────────────────────────────────────────────────

function sendJson(res: ServerResponse, status: number, body: unknown): void {
  const payload = JSON.stringify(body);
  res.writeHead(status, { "content-type": "application/json" });
  res.end(payload);
}

async function handleRequest(env: Env, req: IncomingMessage, res: ServerResponse): Promise<void> {
  const url = new URL(req.url ?? "/", "http://localhost");
  const auth = req.headers.authorization;

  if (req.method === "GET" && url.pathname === "/") {
    res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
    res.end(indexHtml(env.basePath));
    return;
  }

  if (req.method === "POST" && url.pathname === "/api/text-me") {
    const userDid = await verifyToken(env, auth);
    console.log(`testbot: text-me from user_did=${userDid}`);
    const bot = await spawnBot(env, userDid);
    const list = botsByUser.get(userDid) ?? [];
    list.push(bot);
    botsByUser.set(userDid, list);
    sendJson(res, 200, { bot });
    return;
  }

  if (req.method === "GET" && url.pathname === "/api/bots") {
    const userDid = await verifyToken(env, auth);
    sendJson(res, 200, { bots: botsByUser.get(userDid) ?? [] });
    return;
  }

  sendJson(res, 404, { error: "not found" });
}

function main(): void {
  loadDotenv();
  const env = readEnv();
  initLogging(env.logLevel);

  if (!env.anthropicApiKey && !env.anthropicAuthToken) {
    console.warn(
      "testbot: no ANTHROPIC_API_KEY / ANTHROPIC_AUTH_TOKEN configured — " +
        "running in echo mode (replies echo your message). Set one in .env " +
        "(see .env.example) for AI replies. Local dev / `make dev-all` needs no key.",
    );
  }

  const server = createServer((req, res) => {
    handleRequest(env, req, res).catch((e) => {
      if (e instanceof HttpError) {
        console.warn(`testbot: ${req.method} ${req.url} → ${e.status}: ${e.message}`);
        sendJson(res, e.status, { error: e.message });
      } else {
        console.error(`testbot: unhandled error on ${req.method} ${req.url}: ${(e as Error).message}`);
        sendJson(res, 500, { error: "internal error" });
      }
    });
  });

  server.listen(env.bindPort, env.bindHost, () => {
    console.log(`testbot: listening on ${env.bindHost}:${env.bindPort} (homeserver ${env.homeserverUrl})`);
  });
}

main();
