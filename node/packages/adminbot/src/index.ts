// actnet adminbot — the canonical first-party bot.
//
// v1 responsibilities (per docs/22-adminbot.md):
//   - Register a bot account at the reserved DID `did:local:adminbot`
//     (server-side default; override via ADMINBOT_DID on the server).
//   - Create the `#admins @ {hostname}` group, invite the DIDs listed in
//     ADMINBOT_INITIAL_ADMINS at bootstrap.
//   - Auto-invite every new human account (AccountJoinedEvent WS push) to
//     every group adminbot is currently an admin of — `#admins` and any
//     other group it's been added to as admin.
//   - Cap the disappearing-messages timer at 4 weeks on any group it's an
//     admin of, enforced when it's added to the group (0/"off" is clamped
//     too). Enforcing on a later timer change awaits a group-state push.
//   - Check daily for a newer Avalanche release (latest GitHub release tag vs
//     the deployment's VERSION file) and post to `#admins` once per new
//     version when behind.
//   - Respond to `/whoami`, `/audit` (refresh+report every group, enforce the
//     timer cap), `/check` (check for a newer release now), and `/help`.
//
// Persistent state:
//   - SQLCipher DB at ADMINBOT_STATE_DIR/store.db — owned by app-core.
//   - JSON sidecar at ADMINBOT_STATE_DIR/state.json — adminbot's own
//     bookkeeping (group id, already-invited initial admins).

import { mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { join } from "node:path";

import {
  AppCore,
  initLogging,
  type AdminEvent,
  type GroupSummary,
  type IncomingEvent,
  type SendTarget,
} from "@actnet/app-core";

// Reserved well-known suffix for the canonical adminbot account. This also
// matches the server's superuser Project slug (ADMINBOT_PROJECT_SLUG), so the
// bootstrap token below both registers the bot and links it into the superuser
// Project — granting admin authority (docs/24).
const ADMINBOT_DID_SUFFIX = "adminbot";
const ADMINBOT_DID = `did:local:${ADMINBOT_DID_SUFFIX}`;
const SUPERUSER_PROJECT_SLUG = "adminbot";

// Maximum disappearing-messages timer adminbot will tolerate on a group it
// admins: 4 weeks. A group's timer of 0 ("off" — messages never expire) is
// treated as exceeding this and clamped down too, so every group adminbot
// admins keeps messages for at most this long.
const MAX_GROUP_EXPIRY_SECS = 4 * 7 * 24 * 60 * 60;

// GitHub repo whose releases define "the latest Avalanche version" — the same
// source the configure page (`web/assets/configure/configure.js`) and the
// deploy bundle (`infra/deploy/bundle/lib/common.sh`) resolve against.
const GH_REPO = "lincolnq/avalanche";
// How often to auto-check for a newer release. Daily is plenty.
const UPDATE_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;

interface AdminbotState {
  adminsGroupId?: string;
  invitedInitialAdmins?: string[];
  /// Latest release tag we've already announced in #admins, so the daily
  /// update check posts once per new version instead of nagging every day.
  lastUpdateNotifiedTag?: string;
}

interface Env {
  serverUrl: string;
  stateDir: string;
  dbPath: string;
  statePath: string;
  dbKey: string;
  initialAdmins: string[];
  logLevel: string;
  sharedSecret?: string;
  /// Path to the deployment's VERSION file (= the current release tag). The
  /// deploy bundle writes `/opt/avalanche/deployments/current/VERSION`; override
  /// for separate-host / dev runs. Missing/unreadable → version checks no-op.
  versionFile: string;
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
    // Bootstrap secret for closed-registration servers (docs/24). Required to
    // register against a closed server; unset/ignored on an open one.
    sharedSecret: process.env.REGISTRATION_SHARED_SECRET || undefined,
    versionFile: process.env.AVALANCHE_VERSION_FILE ?? "/opt/avalanche/deployments/current/VERSION",
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

async function loginOrRegister(env: Env): Promise<AppCore> {
  // Register on first run, re-login thereafter. app-core decides which based
  // on whether the store already holds an account (including the empty-DB-from-
  // a-failed-registration case) — adminbot only supplies the reserved DID.
  // Bootstrap token naming the superuser Project: registers the bot (against a
  // closed server) and links it into the superuser Project, granting admin
  // authority. Only consulted on first-run registration; ignored on re-login.
  const inviteToken = env.sharedSecret
    ? AppCore.bootstrapToken(env.serverUrl, env.sharedSecret, SUPERUSER_PROJECT_SLUG)
    : undefined;
  const core = await AppCore.loginOrCreateBot(
    env.serverUrl,
    env.dbPath,
    env.dbKey,
    "Adminbot",
    ADMINBOT_DID_SUFFIX,
    inviteToken,
  );
  // Identity policy is ours, not the core's: the store must belong to the
  // reserved adminbot DID. A mismatch means this state dir was created by a
  // different bot, or the server handed back an unexpected DID.
  if (core.did() !== ADMINBOT_DID) {
    throw new Error(
      `local store DID (${core.did()}) is not the reserved adminbot DID ` +
        `(${ADMINBOT_DID}); this state dir belongs to a different bot`,
    );
  }
  return core;
}

async function withRetry<T>(label: string, fn: () => Promise<T>): Promise<T> {
  // Race against server startup in dev-all and against transient errors.
  let delayMs = 500;
  for (;;) {
    try {
      return await fn();
    } catch (e) {
      console.error(`adminbot: ${label} failed: ${(e as Error).message}; retrying in ${delayMs}ms`);
      await new Promise((r) => setTimeout(r, delayMs));
      delayMs = Math.min(delayMs * 2, 30_000);
    }
  }
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
    if (did === ADMINBOT_DID) continue;
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
  env: Env,
  groupId: string,
  event: IncomingEvent,
): Promise<void> {
  // Being added to a group is interesting on its own: app-core auto-accepts
  // the invite (so we're already a full member by the time this fires), and
  // any group we're an admin of becomes an auto-invite target for new
  // server-joiners (see handleAdminEvent). We also enforce the expiry cap
  // here — no accept needed.
  if (event.kind === "groupInvite") {
    const { groupId: gid, inviterDid } = event.groupInvite;
    console.log(`adminbot: added to group ${gid} by ${inviterDid}`);
    await enforceExpiryCap(core, gid);
    return;
  }
  if (event.kind !== "message") return;
  const msg = event.message;
  if (msg.senderDid === ADMINBOT_DID) return;
  // Slash commands are accepted in #admins and in 1:1 DMs with the bot.
  // Replies always go back through the same channel (group → group send,
  // DM → DM).
  const inAdminsGroup = msg.groupId === groupId;
  const isDm = msg.groupId == null;
  if (!inAdminsGroup && !isDm) return;
  await handleCommand(
    core,
    env,
    inAdminsGroup ? { kind: "group", groupId } : { kind: "dm", recipientDid: msg.senderDid },
    msg.senderDid,
    msg.body.trim(),
  );
}

async function handleAdminEvent(
  core: AppCore,
  event: AdminEvent,
): Promise<void> {
  if (event.kind !== "accountJoined") return;
  const { did } = event.accountJoined;
  if (did === ADMINBOT_DID) return;

  // Only humans get auto-invited. Every account registration fires this
  // event — including bots (e.g. testbot spins up a fresh bot account on each
  // "Text Me"). Inviting them would fill groups with bots and fan a Sender
  // Key out to every member on each invite, so skip any bot account.
  let isBot: boolean;
  try {
    isBot = (await core.getAccountInfo(did)).isBot;
  } catch (e) {
    console.error(`adminbot: getAccountInfo(${did}) failed: ${(e as Error).message}; skipping`);
    return;
  }
  if (isBot) {
    console.log(`adminbot: new account ${did} is a bot — not auto-inviting`);
    return;
  }

  await inviteToAdminGroups(core, did);

  // Send a 1:1 welcome DM. Goes over the same sealed-sender channel the
  // GroupContext invite opens, so it works regardless of whether the
  // recipient has accepted any group invite yet.
  try {
    await core.sendDm(
      did,
      "Welcome! You've been added to this server's groups. Type /help to see what I can do.",
    );
    console.log(`adminbot: sent welcome DM to ${did}`);
  } catch (e) {
    console.error(`adminbot: welcome DM to ${did} failed: ${(e as Error).message}`);
  }
}

// Invite a new server-joiner into every group adminbot is currently an admin
// of. The admin check is live (a group's invite policy defaults to admin-only,
// and the bot may only have been added as a plain member) — non-admin groups
// are skipped. #admins is just one such group: adminbot founded it, so it's
// always admin there. Per-group failures are logged and don't abort the rest.
async function inviteToAdminGroups(core: AppCore, did: string): Promise<void> {
  let groupIds: string[];
  try {
    groupIds = await core.listGroups();
  } catch (e) {
    console.error(`adminbot: listGroups failed: ${(e as Error).message}; skipping invites`);
    return;
  }
  for (const gid of groupIds) {
    let summary;
    try {
      summary = await core.fetchGroupState(gid);
    } catch (e) {
      console.error(`adminbot: fetchGroupState(${gid}) failed: ${(e as Error).message}; skipping`);
      continue;
    }
    const me = summary.members.find((m) => m.did === ADMINBOT_DID);
    if (me?.role !== "admin") continue; // not an admin here — leave it alone
    if (summary.members.some((m) => m.did === did)) continue; // already a member
    try {
      console.log(`adminbot: inviting ${did} to ${gid} ("${summary.title}")`);
      await core.inviteMember(gid, did, "member");
    } catch (e) {
      console.error(`adminbot: invite of ${did} to ${gid} failed: ${(e as Error).message}`);
    }
  }
}

// Clamp a group's disappearing-messages timer to MAX_GROUP_EXPIRY_SECS if it
// exceeds it (including "off" / 0, which means never-expire).
function expiryExceedsCap(seconds: number): boolean {
  // 0 = off (never expires) → longer than any finite max, so it exceeds too.
  return seconds === 0 || seconds > MAX_GROUP_EXPIRY_SECS;
}

// Clamp a group's timer to the cap when adminbot is an admin (modify_expiry
// defaults to admin-only) and the current timer exceeds it. Operates on an
// already-fetched summary so callers don't double-fetch. Returns true iff a
// clamp was applied.
async function clampExpiryIfNeeded(core: AppCore, summary: GroupSummary): Promise<boolean> {
  const me = summary.members.find((m) => m.did === ADMINBOT_DID);
  if (me?.role !== "admin") return false;
  if (!expiryExceedsCap(summary.expirySeconds)) return false;
  try {
    console.log(
      `adminbot: clamping expiry of ${summary.groupId} ("${summary.title}") ` +
        `from ${summary.expirySeconds}s to ${MAX_GROUP_EXPIRY_SECS}s`,
    );
    await core.setGroupExpiry(summary.groupId, MAX_GROUP_EXPIRY_SECS);
    return true;
  } catch (e) {
    console.error(`adminbot: clamping expiry of ${summary.groupId} failed: ${(e as Error).message}`);
    return false;
  }
}

// Refresh a single group and clamp its timer if needed. Called when adminbot is
// added to a group; enforcing on a *later* timer change needs a group-state
// push that doesn't exist yet (see docs/02-todos-deferred.md).
async function enforceExpiryCap(core: AppCore, groupId: string): Promise<void> {
  let summary: GroupSummary;
  try {
    summary = await core.fetchGroupState(groupId);
  } catch (e) {
    console.error(`adminbot: fetchGroupState(${groupId}) failed: ${(e as Error).message}; skipping expiry cap`);
    return;
  }
  await clampExpiryIfNeeded(core, summary);
}

// Render a disappearing-messages timer (seconds) as a short human label,
// e.g. 0 -> "off", 604800 -> "1 week".
function formatTimer(seconds: number): string {
  if (seconds === 0) return "off";
  const units: Array<[number, string]> = [
    [7 * 24 * 60 * 60, "week"],
    [24 * 60 * 60, "day"],
    [60 * 60, "hour"],
    [60, "minute"],
    [1, "second"],
  ];
  for (const [size, name] of units) {
    if (seconds % size === 0) {
      const n = seconds / size;
      return `${n} ${name}${n === 1 ? "" : "s"}`;
    }
  }
  return `${seconds} seconds`;
}

// `/audit`: refresh every group adminbot is in and report, per group, whether
// it sees itself as admin, member/admin counts, and the timer — clamping the
// timer to the 4-week cap where it's an admin and the timer exceeds it.
async function runAudit(core: AppCore, channel: SendTarget): Promise<void> {
  let groupIds: string[];
  try {
    groupIds = await core.listGroups();
  } catch (e) {
    await core.send(channel, `audit failed: couldn't list groups (${(e as Error).message})`);
    return;
  }
  if (groupIds.length === 0) {
    await core.send(channel, "audit: I'm not in any groups.");
    return;
  }
  const lines = [`Audit — ${groupIds.length} group${groupIds.length === 1 ? "" : "s"}:`];
  for (const gid of groupIds) {
    let s: GroupSummary;
    try {
      s = await core.fetchGroupState(gid); // refresh from server
    } catch (e) {
      lines.push(`• ${gid}: fetch failed (${(e as Error).message})`);
      continue;
    }
    const isAdmin = s.members.find((m) => m.did === ADMINBOT_DID)?.role === "admin";
    const adminCount = s.members.filter((m) => m.role === "admin").length;
    const before = s.expirySeconds;
    const clamped = await clampExpiryIfNeeded(core, s);
    const timer = clamped ? formatTimer(MAX_GROUP_EXPIRY_SECS) : formatTimer(before);
    const clampNote = clamped ? ` (clamped from ${formatTimer(before)})` : "";
    lines.push(
      `• "${s.title || "(untitled)"}" — admin: ${isAdmin ? "yes" : "no"} | ` +
        `members: ${s.members.length} (${adminCount} admin${adminCount === 1 ? "" : "s"}) | ` +
        `timer: ${timer}${clampNote}`,
    );
  }
  await core.send(channel, lines.join("\n"));
}

// Our running release tag, read from the deployment's VERSION file. null if it
// doesn't exist or is unreadable (dev runs, separate hosts without the file).
function readCurrentVersion(env: Env): string | null {
  try {
    const v = readFileSync(env.versionFile, "utf8").trim();
    return v.length > 0 ? v : null;
  } catch {
    return null;
  }
}

// Newest published release tag from GitHub (prereleases included, newest-first
// — same `/releases` list the configure page and deploy bundle use). null on
// any network/parse error so callers can degrade gracefully.
async function fetchLatestRelease(): Promise<string | null> {
  try {
    const r = await fetch(`https://api.github.com/repos/${GH_REPO}/releases?per_page=10`, {
      headers: { Accept: "application/vnd.github+json" },
    });
    if (!r.ok) {
      console.error(`adminbot: GitHub releases fetch failed: HTTP ${r.status}`);
      return null;
    }
    const releases = (await r.json()) as Array<{ tag_name?: string }>;
    return releases.find((rel) => typeof rel.tag_name === "string")?.tag_name ?? null;
  } catch (e) {
    console.error(`adminbot: GitHub releases fetch error: ${(e as Error).message}`);
    return null;
  }
}

interface UpdateStatus {
  current: string | null;
  latest: string | null;
}

async function checkForUpdate(env: Env): Promise<UpdateStatus> {
  return { current: readCurrentVersion(env), latest: await fetchLatestRelease() };
}

// Daily auto-check: if we're behind the latest release, post to #admins — once
// per new version (deduped via state.lastUpdateNotifiedTag), not every day.
async function runDailyUpdateCheck(
  core: AppCore,
  env: Env,
  state: AdminbotState,
  adminsGroupId: string,
): Promise<void> {
  const { current, latest } = await checkForUpdate(env);
  if (!current) {
    console.log(`adminbot: update check skipped — no readable VERSION at ${env.versionFile}`);
    return;
  }
  if (!latest) return; // couldn't reach GitHub; already logged
  if (latest === current) return; // up to date
  if (state.lastUpdateNotifiedTag === latest) return; // already announced this one

  console.log(`adminbot: update available — running ${current}, latest ${latest}; notifying #admins`);
  try {
    await core.send(
      { kind: "group", groupId: adminsGroupId },
      `A newer Avalanche release is available: ${latest} (running ${current}). ` +
        "An admin can upgrade the server with `avalanche-update`.",
    );
    state.lastUpdateNotifiedTag = latest;
    saveState(env.statePath, state);
  } catch (e) {
    console.error(`adminbot: update notification failed: ${(e as Error).message}`);
  }
}

// `/check`: run the version check now and report the result to the invoking
// channel (always replies, unlike the deduped daily check).
async function runCheckCommand(core: AppCore, env: Env, channel: SendTarget): Promise<void> {
  const { current, latest } = await checkForUpdate(env);
  let msg: string;
  if (!latest) {
    msg = "Couldn't reach GitHub to check for the latest release.";
  } else if (!current) {
    msg = `Latest Avalanche release is ${latest}. (I can't tell what version I'm running — no readable VERSION file.)`;
  } else if (current === latest) {
    msg = `Up to date: running ${current} (the latest release).`;
  } else {
    msg = `Update available: latest is ${latest}, I'm running ${current}. An admin can upgrade with \`avalanche-update\`.`;
  }
  await core.send(channel, msg);
}

async function handleCommand(
  core: AppCore,
  env: Env,
  channel: SendTarget,
  senderDid: string,
  body: string,
): Promise<void> {
  if (!body.startsWith("/")) return;
  const [cmd] = body.split(/\s+/, 1);
  switch (cmd) {
    case "/whoami":
      await core.send(channel, `${senderDid} (admin)`);
      break;
    case "/audit":
      await runAudit(core, channel);
      break;
    case "/check":
      await runCheckCommand(core, env, channel);
      break;
    case "/help":
      await core.send(
        channel,
        [
          "Available commands:",
          "  /whoami    echo your DID",
          "  /audit     get status of all groups",
          "  /check     check now for a newer Avalanche release",
          "  /help      show this help",
        ].join("\n"),
      );
      break;
    default:
      await core.send(channel, `unknown command: ${cmd}. Try /help.`);
  }
}

async function run(): Promise<void> {
  const env = readEnv();
  initLogging(env.logLevel);

  const state: AdminbotState = loadState(env.statePath) ?? {};
  const core = await withRetry("login/register", () => loginOrRegister(env));
  console.log(`adminbot: started (did=${core.did()})`);

  const groupId = await withRetry("ensure #admins group", () =>
    ensureAdminsGroup(core, env, state),
  );
  await inviteInitialAdmins(core, env, state, groupId);

  console.log(`adminbot: listening for events on ${groupId}`);

  const messagesLoop = (async () => {
    for await (const event of core.events()) {
      handleMessage(core, env, groupId, event).catch((e) => {
        console.error(`adminbot: message handler error: ${(e as Error).message}`);
      });
    }
  })();

  const adminLoop = (async () => {
    for await (const event of core.adminEvents()) {
      handleAdminEvent(core, event).catch((e) => {
        console.error(`adminbot: admin handler error: ${(e as Error).message}`);
      });
    }
  })();

  // Daily Avalanche-update check: runs once at startup, then every 24h. Posts
  // to #admins once per newly-detected release (see runDailyUpdateCheck).
  const updateLoop = (async () => {
    for (;;) {
      try {
        await runDailyUpdateCheck(core, env, state, groupId);
      } catch (e) {
        console.error(`adminbot: update check error: ${(e as Error).message}`);
      }
      await new Promise((r) => setTimeout(r, UPDATE_CHECK_INTERVAL_MS));
    }
  })();

  await Promise.all([messagesLoop, adminLoop, updateLoop]);
}

run().catch((e) => {
  console.error(`adminbot: fatal: ${(e as Error).message}`);
  process.exit(1);
});
