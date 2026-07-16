// actnet testbot - the first demo Project on the platform.
//
// A standalone HTTP service that serves a tiny web UI and spins up AI chatbot
// accounts on demand. Each bot is a full Signal-protocol participant: it
// registers on the homeserver, holds its own identity keys, and sends/receives
// encrypted DMs via `@theavalanche/app-core`. Bots converse using Claude Haiku; an
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

import qrcode from "qrcode-generator";

import { AppCore, initLogging, type SendTarget } from "@theavalanche/app-core";

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
  // OAuth login demo (docs/25).
  publicUrl: string;
  oauthClientId: string;
  authorizeUrl: string;
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
    // The externally-reachable base URL for this service — how your PHONE
    // reaches it (e.g. "http://192.168.1.50:3001"). Used to derive the OAuth
    // redirect_uri, which must exactly match what's registered in the
    // homeserver's PROJECTS config. Defaults to the local bind (fine only when
    // the phone is the same machine, which it usually isn't).
    publicUrl: (process.env.TESTBOT_PUBLIC_URL ?? `http://${bindHost === "0.0.0.0" ? "localhost" : bindHost}:${bindPort}`).replace(/\/$/, ""),
    // OAuth client id this demo registers as (docs/25). Must match the
    // `client_id` in the homeserver's PROJECTS entry for this Project.
    oauthClientId: process.env.TESTBOT_OAUTH_CLIENT_ID ?? "testbot",
    // The app's `authorize` Universal Link (the app is the authorization
    // endpoint; docs/25). Fixed to the domain the app claims via AASA.
    authorizeUrl: process.env.TESTBOT_AUTHORIZE_URL ?? "https://go.theavalanche.net/authorize",
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
  return verifyProjectToken(env, token);
}

/**
 * Resolve a project token (including an OAuth access token, which IS a project
 * token — docs/25) to its DID via the homeserver's verify endpoint. The
 * homeserver is the sole authority; we just relay the answer.
 */
async function verifyProjectToken(env: Env, token: string): Promise<string> {
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

/** Read a request body to a string (bounded to a sane size for our small JSON). */
function readBody(req: IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    let data = "";
    req.on("data", (chunk) => {
      data += chunk;
      if (data.length > 64 * 1024) reject(new HttpError(413, "body too large"));
    });
    req.on("end", () => resolve(data));
    req.on("error", reject);
  });
}

/**
 * Exchange an OAuth authorization code (same-device front-end, docs/25) for the
 * caller's DID: POST the code + PKCE verifier to the homeserver token endpoint,
 * then resolve the returned access token to a DID.
 */
async function exchangeOauthCode(
  env: Env,
  code: string,
  codeVerifier: string,
  redirectUri: string,
): Promise<string> {
  const form = new URLSearchParams({
    grant_type: "authorization_code",
    code,
    redirect_uri: redirectUri,
    code_verifier: codeVerifier,
    client_id: env.oauthClientId,
  });
  let resp: Response;
  try {
    resp = await fetch(`${env.homeserverUrl}/v1/oauth/token`, {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded" },
      body: form.toString(),
    });
  } catch (e) {
    throw new HttpError(502, `token endpoint request failed: ${(e as Error).message}`);
  }
  if (!resp.ok) {
    throw new HttpError(401, `token exchange rejected: ${resp.status} ${await resp.text()}`);
  }
  const body = (await resp.json()) as { access_token?: string };
  if (!body.access_token) {
    throw new HttpError(502, "token response missing 'access_token'");
  }
  return verifyProjectToken(env, body.access_token);
}

interface DeviceAuthResponse {
  device_code: string;
  user_code: string;
  verification_uri: string;
  verification_uri_complete: string;
  expires_in: number;
  interval: number;
}

/** Start a cross-device (device-grant) login: ask the homeserver for a
 *  device_code/user_code pair the user approves on their phone (docs/25). */
async function startDeviceAuth(env: Env): Promise<DeviceAuthResponse> {
  const form = new URLSearchParams({ client_id: env.oauthClientId });
  let resp: Response;
  try {
    resp = await fetch(`${env.homeserverUrl}/v1/oauth/device_authorization`, {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded" },
      body: form.toString(),
    });
  } catch (e) {
    throw new HttpError(502, `device_authorization failed: ${(e as Error).message}`);
  }
  if (!resp.ok) {
    throw new HttpError(401, `device_authorization rejected: ${resp.status} ${await resp.text()}`);
  }
  return (await resp.json()) as DeviceAuthResponse;
}

type DevicePoll =
  | { status: "pending" }
  | { status: "slow_down" }
  | { status: "error"; error: string }
  | { status: "done"; did: string };

/** Poll the homeserver token endpoint for a device grant (docs/25 / RFC 8628).
 *  Returns a coarse status the browser can act on. */
async function pollDeviceToken(env: Env, deviceCode: string): Promise<DevicePoll> {
  const form = new URLSearchParams({
    grant_type: "urn:ietf:params:oauth:grant-type:device_code",
    device_code: deviceCode,
    client_id: env.oauthClientId,
  });
  const resp = await fetch(`${env.homeserverUrl}/v1/oauth/token`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: form.toString(),
  });
  const body = (await resp.json().catch(() => ({}))) as { access_token?: string; error?: string };
  if (resp.ok && body.access_token) {
    const did = await verifyProjectToken(env, body.access_token);
    return { status: "done", did };
  }
  if (body.error === "authorization_pending") return { status: "pending" };
  if (body.error === "slow_down") return { status: "slow_down" };
  return { status: "error", error: body.error ?? `http ${resp.status}` };
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

// ── OAuth login demo page (docs/25) ──────────────────────────────────────────

/** The exact redirect_uri this demo uses — must be registered in the
 *  homeserver's PROJECTS entry for `oauthClientId`. */
function loginRedirectUri(env: Env): string {
  return `${env.publicUrl}${env.basePath}login`;
}

/**
 * A standalone "Sign in with Avalanche" relying-party page (docs/25). Unlike
 * the token-in-URL webview page (`/`), you open THIS in a browser directly:
 * tapping "Text Me" runs the same-device authorization-code + PKCE flow — the
 * app opens for consent, redirects back here with a code, our backend exchanges
 * it for your DID, and a bot DMs you.
 */
const loginHtml = (env: Env) => `<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <base href="${env.basePath}">
    <title>Sign in with Avalanche — Demo</title>
    <style>
        :root { --brand: #2f6fed; --brand-dark: #1f57c9; }
        * { box-sizing: border-box; }
        body { font-family: -apple-system, system-ui, sans-serif; margin: 0; min-height: 100vh;
               display: flex; align-items: center; justify-content: center;
               background: linear-gradient(160deg, #eef2fb 0%, #e6ecf8 100%); color: #1a1f36; }
        .card { background: #fff; width: 360px; max-width: calc(100vw - 32px); padding: 40px 32px;
                border-radius: 16px; box-shadow: 0 12px 40px rgba(20,30,60,.12); text-align: center; }
        .logo { width: 56px; height: 56px; border-radius: 14px; margin: 0 auto 18px; background: var(--brand);
                display: flex; align-items: center; justify-content: center; color: #fff; font-size: 30px; }
        h1 { font-size: 22px; margin: 0 0 4px; }
        .sub { color: #6b7280; font-size: 15px; margin: 0 0 28px; }
        .btn { display: inline-flex; align-items: center; justify-content: center; width: 100%;
               font-size: 16px; font-weight: 600; padding: 14px 20px; cursor: pointer; border: none;
               border-radius: 10px; background: var(--brand); color: #fff; text-decoration: none; }
        .btn:hover { background: var(--brand-dark); }
        .btn.disabled { background: #b9c2d6; pointer-events: none; }
        .status { margin-top: 18px; color: #6b7280; font-size: 14px; min-height: 20px; }
        .modal-backdrop { position: fixed; inset: 0; background: rgba(15,23,42,.55);
                          display: flex; align-items: center; justify-content: center; padding: 16px; }
        .modal { position: relative; background: #fff; border-radius: 16px; padding: 28px; width: 320px;
                 max-width: 100%; text-align: center; box-shadow: 0 20px 60px rgba(0,0,0,.25); }
        .modal h2 { font-size: 18px; margin: 0 0 4px; }
        .qr { width: 240px; height: 240px; margin: 18px auto 0; display: block; border-radius: 8px; }
        .close { position: absolute; top: 10px; right: 14px; background: none; border: none;
                 font-size: 26px; line-height: 1; color: #9aa3b2; cursor: pointer; padding: 0; }
    </style>
</head>
<body>
    <div class="card">
        <div class="logo">❄</div>
        <h1>Demo App</h1>
        <p class="sub">Continue with your Avalanche account</p>
        <a id="login" class="btn disabled" href="#">Log in with Avalanche</a>
        <div id="status" class="status"></div>
    </div>

    <div id="modal" class="modal-backdrop" style="display:none">
        <div class="modal">
            <button id="modalclose" class="close" aria-label="Close">&times;</button>
            <h2>Log in with Avalanche</h2>
            <p class="sub" style="margin:0">Scan with your phone, it will launch Avalanche, then approve.</p>
            <img id="qrimg" class="qr" alt="Login QR code" width="240" height="240">
            <div id="modalstatus" class="status"></div>
        </div>
    </div>
    <script>
        const HOMESERVER = ${JSON.stringify(env.homeserverUrl)};
        const CLIENT_ID = ${JSON.stringify(env.oauthClientId)};
        const AUTHORIZE_URL = ${JSON.stringify(env.authorizeUrl)};
        const REDIRECT_URI = location.origin + location.pathname;

        function b64url(bytes) {
            let s = btoa(String.fromCharCode.apply(null, bytes));
            return s.replace(/\\+/g, '-').replace(/\\//g, '_').replace(/=+$/, '');
        }
        function randB64(n) {
            const a = new Uint8Array(n); crypto.getRandomValues(a); return b64url(a);
        }

        // Pure-JS SHA-256 (Uint8Array -> Uint8Array[32]). Used because
        // crypto.subtle is only available in secure contexts (HTTPS / localhost),
        // and this demo is often served over plain http on a LAN/Tailscale host.
        function sha256Bytes(msg) {
            const K = new Uint32Array([
                0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
                0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
                0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
                0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
                0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
                0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
                0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
                0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2]);
            let h0=0x6a09e667,h1=0xbb67ae85,h2=0x3c6ef372,h3=0xa54ff53a,h4=0x510e527f,h5=0x9b05688c,h6=0x1f83d9ab,h7=0x5be0cd19;
            const l = msg.length, bitLen = l * 8;
            const withOne = l + 1;
            const pad = (56 - (withOne % 64) + 64) % 64;
            const total = withOne + pad + 8;
            const m = new Uint8Array(total);
            m.set(msg); m[l] = 0x80;
            const hi = Math.floor(bitLen / 0x100000000), lo = bitLen >>> 0;
            m[total-8]=(hi>>>24)&0xff; m[total-7]=(hi>>>16)&0xff; m[total-6]=(hi>>>8)&0xff; m[total-5]=hi&0xff;
            m[total-4]=(lo>>>24)&0xff; m[total-3]=(lo>>>16)&0xff; m[total-2]=(lo>>>8)&0xff; m[total-1]=lo&0xff;
            const w = new Uint32Array(64);
            const rotr = (x,n) => (x>>>n)|(x<<(32-n));
            for (let off=0; off<total; off+=64) {
                for (let i=0;i<16;i++){ w[i]=((m[off+i*4]<<24)|(m[off+i*4+1]<<16)|(m[off+i*4+2]<<8)|(m[off+i*4+3]))>>>0; }
                for (let i=16;i<64;i++){
                    const s0=rotr(w[i-15],7)^rotr(w[i-15],18)^(w[i-15]>>>3);
                    const s1=rotr(w[i-2],17)^rotr(w[i-2],19)^(w[i-2]>>>10);
                    w[i]=(w[i-16]+s0+w[i-7]+s1)>>>0;
                }
                let a=h0,b=h1,c=h2,d=h3,e=h4,f=h5,g=h6,h=h7;
                for(let i=0;i<64;i++){
                    const S1=rotr(e,6)^rotr(e,11)^rotr(e,25);
                    const ch=(e&f)^((~e)&g);
                    const t1=(h+S1+ch+K[i]+w[i])>>>0;
                    const S0=rotr(a,2)^rotr(a,13)^rotr(a,22);
                    const maj=(a&b)^(a&c)^(b&c);
                    const t2=(S0+maj)>>>0;
                    h=g;g=f;f=e;e=(d+t1)>>>0;d=c;c=b;b=a;a=(t1+t2)>>>0;
                }
                h0=(h0+a)>>>0;h1=(h1+b)>>>0;h2=(h2+c)>>>0;h3=(h3+d)>>>0;h4=(h4+e)>>>0;h5=(h5+f)>>>0;h6=(h6+g)>>>0;h7=(h7+h)>>>0;
            }
            const out = new Uint8Array(32), hs=[h0,h1,h2,h3,h4,h5,h6,h7];
            for(let i=0;i<8;i++){ out[i*4]=(hs[i]>>>24)&0xff; out[i*4+1]=(hs[i]>>>16)&0xff; out[i*4+2]=(hs[i]>>>8)&0xff; out[i*4+3]=hs[i]&0xff; }
            return out;
        }
        async function sha256b64(v) {
            const data = new TextEncoder().encode(v);
            if (typeof crypto !== 'undefined' && crypto.subtle && crypto.subtle.digest) {
                const d = await crypto.subtle.digest('SHA-256', data);
                return b64url(new Uint8Array(d));
            }
            return b64url(sha256Bytes(data));
        }

        let deviceStarted = false;
        let pollCancelled = false;

        // A phone (which could have the app installed) vs a desktop. A web page
        // can't reliably detect an installed app, so we split by device type:
        // phones tap-through to launch the app; desktops pop the QR modal.
        function isMobileDevice() {
            const ua = navigator.userAgent || '';
            if (/iphone|ipad|ipod|android/i.test(ua)) return true;
            // iPadOS 13+ reports as Mac; distinguish by touch support.
            if (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1) return true;
            return false;
        }

        function openModal() {
            pollCancelled = false;
            document.getElementById('modal').style.display = 'flex';
            if (!deviceStarted) { deviceStarted = true; startDevice(); }
        }
        function closeModal() {
            pollCancelled = true;
            deviceStarted = false;
            document.getElementById('modal').style.display = 'none';
        }

        // Cross-device (device-grant) flow shown in the modal: a QR the user
        // scans with their Avalanche app, polled until they approve (docs/25).
        async function startDevice() {
            const status = document.getElementById('modalstatus');
            status.textContent = 'Preparing…';
            let data;
            try {
                const resp = await fetch('api/oauth/device/start', { method: 'POST' });
                if (!resp.ok) { status.textContent = 'Failed to start: ' + resp.status; return; }
                data = await resp.json();
            } catch (e) { status.textContent = 'Error: ' + e.message; return; }
            document.getElementById('qrimg').src = data.qr;
            status.textContent = 'Waiting for approval…';
            poll(data.device_code, Math.max(2, data.interval || 5) * 1000);
        }

        function poll(deviceCode, intervalMs) {
            setTimeout(async () => {
                if (pollCancelled) return;
                try {
                    const r = await fetch('api/oauth/device/poll', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ device_code: deviceCode }),
                    });
                    const d = await r.json();
                    if (pollCancelled) return;
                    if (d.status === 'done') {
                        closeModal();
                        document.getElementById('login').style.display = 'none';
                        document.getElementById('status').textContent = 'Signed in! The bot is texting your phone.';
                        return;
                    }
                    if (d.status === 'error') { document.getElementById('modalstatus').textContent = 'Sign-in failed: ' + d.error; return; }
                    if (d.status === 'slow_down') intervalMs += 5000;
                    poll(deviceCode, intervalMs);
                } catch (e) { document.getElementById('modalstatus').textContent = 'Error: ' + e.message; }
            }, intervalMs);
        }

        // On load: if we came back from the app with ?code, finish the exchange.
        // Otherwise arm the login button (mobile: launch the app; desktop: modal).
        async function init() {
            const params = new URLSearchParams(location.search);
            const status = document.getElementById('status');
            const login = document.getElementById('login');

            if (params.get('code')) {
                login.style.display = 'none';
                const expected = localStorage.getItem('oauth_state');
                const verifier = localStorage.getItem('pkce_verifier');
                if (!verifier || params.get('state') !== expected) {
                    status.textContent = 'Login state mismatch — please start again.';
                    return;
                }
                status.textContent = 'Signing you in…';
                try {
                    const resp = await fetch('api/oauth/exchange', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ code: params.get('code'), code_verifier: verifier, redirect_uri: REDIRECT_URI }),
                    });
                    if (!resp.ok) { status.textContent = 'Sign-in failed: ' + resp.status + ' ' + (await resp.text()); return; }
                    const data = await resp.json();
                    localStorage.removeItem('pkce_verifier'); localStorage.removeItem('oauth_state');
                    status.textContent = 'Signed in! Opening the conversation…';
                    setTimeout(() => { location.href = 'https://go.theavalanche.net/conversation/' + data.bot.did; }, 1200);
                } catch (e) {
                    status.textContent = 'Error: ' + e.message;
                }
                return;
            }

            // Prepare PKCE + the authorize URL so a tap launches the app on mobile.
            const verifier = randB64(32);
            const state = randB64(16);
            localStorage.setItem('pkce_verifier', verifier);
            localStorage.setItem('oauth_state', state);
            const challenge = await sha256b64(verifier);
            const u = new URL(AUTHORIZE_URL);
            u.searchParams.set('client_id', CLIENT_ID);
            u.searchParams.set('redirect_uri', REDIRECT_URI);
            u.searchParams.set('code_challenge', challenge);
            u.searchParams.set('code_challenge_method', 'S256');
            u.searchParams.set('state', state);
            u.searchParams.set('server_url', HOMESERVER);
            u.searchParams.set('scope', 'login');
            login.href = u.toString();
            login.classList.remove('disabled');

            // Mobile: let the tap navigate to the Universal Link (launches the
            // app). Desktop: no app to launch, so pop the QR modal instead.
            login.addEventListener('click', function (ev) {
                if (!isMobileDevice()) { ev.preventDefault(); openModal(); }
            });
            document.getElementById('modalclose').onclick = closeModal;
            document.getElementById('modal').addEventListener('click', function (ev) {
                if (ev.target === document.getElementById('modal')) closeModal();
            });
        }

        init();
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

  // OAuth "Sign in with Avalanche" demo page (docs/25). Visited directly in a
  // browser (not opened by the app), unlike `/`.
  if (req.method === "GET" && url.pathname === "/login") {
    res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
    res.end(loginHtml(env));
    return;
  }

  // Backend leg of the OAuth code exchange: browser posts the code + PKCE
  // verifier, we resolve the DID and spin up a bot that DMs the user.
  if (req.method === "POST" && url.pathname === "/api/oauth/exchange") {
    const body = JSON.parse((await readBody(req)) || "{}") as {
      code?: string;
      code_verifier?: string;
      redirect_uri?: string;
    };
    if (!body.code || !body.code_verifier || !body.redirect_uri) {
      throw new HttpError(400, "missing code / code_verifier / redirect_uri");
    }
    const userDid = await exchangeOauthCode(env, body.code, body.code_verifier, body.redirect_uri);
    console.log(`testbot: oauth login from user_did=${userDid}`);
    const bot = await spawnBot(env, userDid);
    const list = botsByUser.get(userDid) ?? [];
    list.push(bot);
    botsByUser.set(userDid, list);
    sendJson(res, 200, { bot });
    return;
  }

  // Cross-device (device-grant) login: start → returns a QR the user scans with
  // their phone (docs/25).
  if (req.method === "POST" && url.pathname === "/api/oauth/device/start") {
    const auth = await startDeviceAuth(env);
    // qrcode-generator: type 0 = auto-size for the data, error-correction "M".
    // createDataURL(cellSize, margin) → a GIF data URL; the <img> is CSS-sized.
    const qrGen = qrcode(0, "M");
    qrGen.addData(auth.verification_uri_complete);
    qrGen.make();
    const qr = qrGen.createDataURL(5, 5);
    sendJson(res, 200, {
      device_code: auth.device_code,
      user_code: auth.user_code,
      verification_uri_complete: auth.verification_uri_complete,
      interval: auth.interval,
      expires_in: auth.expires_in,
      qr,
    });
    return;
  }

  // Cross-device login: poll until the phone approves, then spin up the bot.
  if (req.method === "POST" && url.pathname === "/api/oauth/device/poll") {
    const body = JSON.parse((await readBody(req)) || "{}") as { device_code?: string };
    if (!body.device_code) throw new HttpError(400, "missing device_code");
    const result = await pollDeviceToken(env, body.device_code);
    if (result.status === "done") {
      console.log(`testbot: device login from user_did=${result.did}`);
      const bot = await spawnBot(env, result.did);
      const list = botsByUser.get(result.did) ?? [];
      list.push(bot);
      botsByUser.set(result.did, list);
      sendJson(res, 200, { status: "done", bot });
    } else {
      sendJson(res, 200, result);
    }
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
    // OAuth login demo (docs/25): print the exact PROJECTS entry to register on
    // the homeserver so this service is a recognized OAuth client. The
    // redirect_uri must match byte-for-byte what the phone browser will visit.
    const redirectUri = loginRedirectUri(env);
    const projectEntry = {
      name: "Testbot",
      url: env.publicUrl,
      description: "Chat with an AI bot",
      client_id: env.oauthClientId,
      redirect_uris: [redirectUri],
      official: true,
    };
    console.log(`testbot: OAuth login demo page → ${env.publicUrl}${env.basePath}login`);
    console.log(
      `testbot: register this OAuth client on the homeserver (PROJECTS env):\n` +
        `  PROJECTS='${JSON.stringify([projectEntry])}'`,
    );
    if (env.publicUrl.includes("localhost")) {
      console.warn(
        "testbot: TESTBOT_PUBLIC_URL is localhost — set it to this machine's " +
          "LAN URL (e.g. http://192.168.x.x:3001) so your phone can reach the " +
          "demo, and re-register the PROJECTS redirect_uri to match.",
      );
    }
  });
}

main();
