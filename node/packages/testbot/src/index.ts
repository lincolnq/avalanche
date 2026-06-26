// actnet testbot — the first demo Project on the platform.
//
// A standalone HTTP service that serves a tiny web UI and spins up AI chatbot
// accounts on demand. Each bot is a full Signal-protocol participant: it
// registers on the homeserver, holds its own identity keys, and sends/receives
// encrypted DMs via `@actnet/app-core`. Bots converse using Claude Haiku
// (falling back to an echo when no API key is configured).
//
// Lifecycle / state:
//   - All bot state is in-memory: the `botsByUser` registry dies when the
//     service restarts. Each bot's SQLCipher store is a throwaway file under
//     the OS temp dir (app-core has no in-memory store binding for node), so
//     restarting the service abandons every bot identity — matching the
//     original Rust testbot's ephemeral semantics.
//   - Each bot runs one `for await` event loop over its own AppCore. Node is
//     single-threaded, so a bot processes its messages sequentially with no
//     locking; libsignal's non-Send constraint that forced dedicated threads
//     in the Rust port simply doesn't apply here (the native module owns its
//     own runtime behind the napi boundary).

import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { AppCore, initLogging, type SendTarget } from "@actnet/app-core";

const OPENING = "Hey! I'm a testbot. Ask me anything.";
const HAIKU_MODEL = "claude-haiku-4-5-20251001";

interface Env {
  homeserverUrl: string;
  anthropicApiKey?: string;
  bindHost: string;
  bindPort: number;
  logLevel: string;
  sharedSecret?: string;
  basePath: string;
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
        a.openbtn { display: inline-block; font-size: 18px; padding: 12px 24px; margin-top: 16px; background: #007AFF; color: white; border-radius: 8px; text-decoration: none; }
        /* Must out-specify a.openbtn (0-1-1), so scope the hide rule to the element+class. */
        a.openbtn.hidden { display: none; }
        #status { margin-top: 16px; color: #666; }
    </style>
</head>
<body>
    <h1>Testbot</h1>
    <p>Tap below to start a conversation with an AI chatbot. The bot will send you an encrypted DM.</p>
    <button id="textme" onclick="textMe()">Text Me</button>
    <div id="status"></div>
    <a id="openlink" class="openbtn hidden" href="#">Click to open the conversation</a>
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
                status.textContent = 'Bot created!';
                // Reveal a real link for the user to tap. A genuine tap is the
                // user gesture both platforms require to hand a verified link off
                // to the app (Android App Link / iOS Universal Link); a JS
                // window.location after the await has no live gesture, so Chrome
                // keeps it in the browser. The tap fixes that on both platforms
                // with one code path — no intent:// / UA sniffing needed.
                const link = document.getElementById('openlink');
                link.href = 'https://go.theavalanche.net/conversation/' + data.bot.did;
                link.classList.remove('hidden');
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
  // Throwaway SQLCipher store in a temp dir — the bot identity is meant to die
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
    const response = await generateResponse(env.anthropicApiKey, conversation, displayName || undefined);
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

/**
 * Ask Claude Haiku for the next reply. With no API key configured, falls back
 * to echoing the user's last message so the demo still works offline.
 */
async function generateResponse(
  apiKey: string | undefined,
  conversation: ConversationMessage[],
  userDisplayName: string | undefined,
): Promise<string> {
  if (!apiKey) return echoResponse(conversation);

  let systemPrompt =
    "You are a friendly chatbot on the actnet platform. Keep your responses " +
    "concise and conversational. You're chatting with an activist — be " +
    "supportive and helpful.";
  if (userDisplayName) {
    systemPrompt += ` The user's display name is ${userDisplayName}.`;
  }

  try {
    const resp = await fetch("https://api.anthropic.com/v1/messages", {
      method: "POST",
      headers: {
        "x-api-key": apiKey,
        "anthropic-version": "2023-06-01",
        "content-type": "application/json",
      },
      body: JSON.stringify({
        model: HAIKU_MODEL,
        max_tokens: 1024,
        system: systemPrompt,
        messages: conversation.map((m) => ({ role: m.role, content: m.content })),
      }),
    });
    if (!resp.ok) {
      console.error(`testbot: claude API error ${resp.status}: ${await resp.text()}`);
      return echoResponse(conversation);
    }
    const body = (await resp.json()) as { content?: Array<{ text?: string }> };
    return body.content?.[0]?.text ?? "I'm having trouble thinking right now. Try again?";
  } catch (e) {
    console.error(`testbot: claude API request failed: ${(e as Error).message}`);
    return echoResponse(conversation);
  }
}

/** Offline fallback: echo the user's most recent message. */
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
  const env = readEnv();
  initLogging(env.logLevel);

  if (!env.anthropicApiKey) {
    console.warn("testbot: ANTHROPIC_API_KEY not set — bots will echo messages instead of using Claude");
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
