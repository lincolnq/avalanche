// actnet adminbot — the canonical first-party bot.
//
// v1 responsibilities (per docs/22-adminbot.md):
//   - Register a `did:local:` bot account, print the DID for the operator
//     to pin as ADMINBOT_DID on the server.
//   - Create the `#admins @ {hostname}` group, invite the DIDs listed in
//     ADMINBOT_INITIAL_ADMINS at bootstrap.
//   - Auto-invite every new account (AccountJoinedEvent WS push) to
//     `#admins`.
//   - Respond to `/whoami` and `/help` in `#admins`.
//
// Persistent state:
//   - SQLCipher DB at ADMINBOT_STATE_DIR/store.db — owned by app-core.
//   - JSON sidecar at ADMINBOT_STATE_DIR/state.json — adminbot's own
//     bookkeeping (group id, master key, already-invited initial admins).

import { mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { join } from "node:path";

import {
  AppCore,
  initLogging,
  type AdminEvent,
  type IncomingEvent,
} from "@actnet/app-core";

interface AdminbotState {
  adminbotDid: string;
  adminsGroupId?: string;
  invitedInitialAdmins?: string[];
}

interface Env {
  serverUrl: string;
  stateDir: string;
  dbPath: string;
  statePath: string;
  dbKey: string;
  initialAdmins: string[];
  logLevel: string;
}

function readEnv(): Env {
  const serverUrl = process.env.ADMINBOT_SERVER_URL;
  if (!serverUrl) {
    throw new Error("ADMINBOT_SERVER_URL is required");
  }
  const stateDir = process.env.ADMINBOT_STATE_DIR ?? "./adminbot-state";
  mkdirSync(stateDir, { recursive: true });
  const initialAdmins =
    process.env.ADMINBOT_INITIAL_ADMINS?.split(",")
      .map((s) => s.trim())
      .filter((s) => s.length > 0) ?? [];
  return {
    serverUrl,
    stateDir,
    dbPath: join(stateDir, "store.db"),
    statePath: join(stateDir, "state.json"),
    dbKey: process.env.ADMINBOT_DB_KEY ?? "",
    initialAdmins,
    logLevel: process.env.ADMINBOT_LOG ?? "info",
  };
}

function loadState(path: string): AdminbotState | null {
  if (!existsSync(path)) return null;
  return JSON.parse(readFileSync(path, "utf8")) as AdminbotState;
}

function saveState(path: string, state: AdminbotState): void {
  writeFileSync(path, JSON.stringify(state, null, 2));
}

function adminsTitle(serverUrl: string): string {
  return `#admins @ ${new URL(serverUrl).hostname}`;
}

async function bootstrap(env: Env): Promise<void> {
  console.log(`adminbot: first run — registering with ${env.serverUrl}`);
  const core = await AppCore.createBotAccount(env.serverUrl, env.dbPath, env.dbKey, "Adminbot");
  const did = core.did();
  saveState(env.statePath, { adminbotDid: did });
  console.log("");
  console.log("─────────────────────────────────────────────────────────────");
  console.log(`  Adminbot DID: ${did}`);
  console.log("");
  console.log("  Bootstrap step 2 of 2:");
  console.log(`    1. Set ADMINBOT_DID=${did} on the homeserver process.`);
  console.log("    2. Restart the homeserver.");
  console.log("    3. Re-run adminbot.");
  console.log("─────────────────────────────────────────────────────────────");
  console.log("");
}

async function ensureAdminsGroup(core: AppCore, env: Env, state: AdminbotState): Promise<string> {
  if (state.adminsGroupId) return state.adminsGroupId;

  const title = adminsTitle(env.serverUrl);
  console.log(`adminbot: creating group "${title}"`);
  const created = await core.createGroup(title, "Server administrators.", 0);
  state.adminsGroupId = created.groupId;
  saveState(env.statePath, state);
  return created.groupId;
}

async function inviteInitialAdmins(
  core: AppCore,
  env: Env,
  state: AdminbotState,
  groupId: string,
): Promise<void> {
  const already = new Set(state.invitedInitialAdmins ?? []);
  for (const did of env.initialAdmins) {
    if (already.has(did)) continue;
    if (did === state.adminbotDid) continue;
    try {
      console.log(`adminbot: inviting initial admin ${did}`);
      await core.inviteMember(groupId, did, "admin");
      already.add(did);
    } catch (e) {
      console.error(`adminbot: failed to invite ${did}: ${(e as Error).message}`);
      // continue — partial success is fine, operator can re-run
    }
  }
  state.invitedInitialAdmins = [...already];
  saveState(env.statePath, state);
}

async function handleMessage(
  core: AppCore,
  groupId: string,
  event: IncomingEvent,
): Promise<void> {
  if (event.kind === "message" && event.message.groupId === groupId) {
    await handleCommand(core, groupId, event.message.senderDid, event.message.body.trim());
  }
}

async function handleAdminEvent(
  core: AppCore,
  state: AdminbotState,
  groupId: string,
  event: AdminEvent,
): Promise<void> {
  if (event.kind === "accountJoined") {
    const { did } = event.accountJoined;
    if (did === state.adminbotDid) return;
    console.log(`adminbot: new account ${did} — inviting to #admins`);
    try {
      await core.inviteMember(groupId, did, "member");
    } catch (e) {
      console.error(`adminbot: invite of ${did} failed: ${(e as Error).message}`);
    }
  }
}

async function handleCommand(
  core: AppCore,
  groupId: string,
  senderDid: string,
  body: string,
): Promise<void> {
  if (!body.startsWith("/")) return;
  const [cmd] = body.split(/\s+/, 1);
  switch (cmd) {
    case "/whoami":
      await core.sendGroupMessage(groupId, `${senderDid} (admin)`);
      break;
    case "/help":
      await core.sendGroupMessage(
        groupId,
        ["Available commands:", "  /whoami    echo your DID", "  /help      show this help"].join("\n"),
      );
      break;
    default:
      await core.sendGroupMessage(groupId, `unknown command: ${cmd}. Try /help.`);
  }
}

async function run(): Promise<void> {
  const env = readEnv();
  initLogging(env.logLevel);

  let state = loadState(env.statePath);
  if (!state) {
    await bootstrap(env);
    return;
  }

  console.log(`adminbot: starting (did=${state.adminbotDid})`);
  const core = await AppCore.login(env.dbPath, env.dbKey);
  if (core.did() !== state.adminbotDid) {
    throw new Error(
      `state.json DID (${state.adminbotDid}) does not match local store DID (${core.did()})`,
    );
  }

  const groupId = await ensureAdminsGroup(core, env, state);
  await inviteInitialAdmins(core, env, state, groupId);

  console.log(`adminbot: listening for events on ${groupId}`);

  const messagesLoop = (async () => {
    for await (const event of core.events()) {
      handleMessage(core, groupId, event).catch((e) => {
        console.error(`adminbot: message handler error: ${(e as Error).message}`);
      });
    }
  })();

  const adminLoop = (async () => {
    for await (const event of core.adminEvents()) {
      handleAdminEvent(core, state, groupId, event).catch((e) => {
        console.error(`adminbot: admin handler error: ${(e as Error).message}`);
      });
    }
  })();

  await Promise.all([messagesLoop, adminLoop]);
}

run().catch((e) => {
  console.error(`adminbot: fatal: ${(e as Error).message}`);
  process.exit(1);
});
