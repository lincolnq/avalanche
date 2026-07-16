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
//   - Announce every new *bot* account in `#admins` with its contact card,
//     so admins can see any bot on the server — except the noisy ephemeral
//     ones (UNANNOUNCED_BOT_NAMES, e.g. testbot). Bots are never auto-invited.
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
  type DecryptedMessage,
  type GroupSummary,
  type IncomingEvent,
  type SendTarget,
} from "@theavalanche/app-core";

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

// Bot display names adminbot will NOT announce in #admins when they join. Every
// account registration fires accountJoined — including bots — and we want admins
// to see the contact card of any bot that joins EXCEPT the noisy ephemeral ones:
// testbot spins up a fresh throwaway bot account on every "Text Me" tap
// (node/packages/testbot/src/index.ts), which would flood #admins. This is a
// deliberate temporary special-case keyed on the well-known display name; the
// durable fix is to announce only bots linked to a Project (their DID appears in
// a project's bot_dids) and drop this list.
const UNANNOUNCED_BOT_NAMES = new Set(["Testbot"]);

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

  // An in-progress interview (e.g. /install-project) runs in a DM and consumes
  // that sender's subsequent DM lines as answers, not commands.
  if (isDm && installInterviews.has(msg.senderDid)) {
    await handleInstallReply(core, env, msg.senderDid, msg.body.trim());
    return;
  }

  await handleCommand(
    core,
    env,
    groupId,
    inAdminsGroup ? { kind: "group", groupId } : { kind: "dm", recipientDid: msg.senderDid },
    msg.senderDid,
    msg.body.trim(),
    msg.sentAt,
  );
}

async function handleAdminEvent(
  core: AppCore,
  event: AdminEvent,
  adminsGroupId: string,
): Promise<void> {
  if (event.kind !== "accountJoined") return;
  const { did } = event.accountJoined;
  if (did === ADMINBOT_DID) return;

  // Only humans get auto-invited. Every account registration fires this
  // event — including bots (e.g. testbot spins up a fresh bot account on each
  // "Text Me"). Inviting them would fill groups with bots and fan a Sender
  // Key out to every member on each invite, so skip auto-invite for any bot.
  // Instead, a bot join is announced in #admins with the bot's contact card
  // (below), except for the noisy ephemeral ones (UNANNOUNCED_BOT_NAMES).
  let info: Awaited<ReturnType<typeof core.getAccountInfo>>;
  try {
    info = await core.getAccountInfo(did);
  } catch (e) {
    console.error(`adminbot: getAccountInfo(${did}) failed: ${(e as Error).message}; skipping`);
    return;
  }
  if (info.isBot) {
    await announceBotJoin(core, adminsGroupId, did, info.displayName);
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

// Announce a newly-joined bot in #admins with its contact card, so admins can
// see (and tap through to) any bot on the server. Skips the well-known noisy
// ephemeral bots (see UNANNOUNCED_BOT_NAMES). The bot's display name is public
// server metadata (getAccountInfo populates it for bots); the contact card is a
// structured, inline SharedContact — the same one People/compose send — carried
// on an otherwise-text message via sendWithAttachments. If the bot registered
// with a Project's setup code, the server has already linked its DID into that
// Project (registration.rs links before it fans the join out), so we name the
// registering Project in the announcement. Failures are logged and swallowed: a
// missed announcement must never wedge the admin-event loop.
async function announceBotJoin(
  core: AppCore,
  adminsGroupId: string,
  did: string,
  displayName?: string,
): Promise<void> {
  const name = displayName?.trim() || did;
  if (displayName && UNANNOUNCED_BOT_NAMES.has(displayName.trim())) {
    console.log(`adminbot: new bot ${did} ("${name}") is ephemeral — not announcing`);
    return;
  }

  // Which installed Project (if any) registered this bot. Best-effort: on any
  // lookup failure we still announce, just without the project attribution.
  let projectName: string | undefined;
  try {
    projectName = (await fetchProjects(core)).find((p) => p.bot_dids.includes(did))?.name;
  } catch (e) {
    console.error(`adminbot: project lookup for ${did} failed: ${(e as Error).message}`);
  }

  const suffix = projectName ? ` (registered by the "${projectName}" project)` : "";
  try {
    console.log(
      `adminbot: announcing new bot ${did} ("${name}")` +
        `${projectName ? ` from project "${projectName}"` : ""} in #admins`,
    );
    await core.sendWithAttachments(
      { kind: "group", groupId: adminsGroupId },
      `A new bot joined the server: ${name}${suffix}`,
      [],
      [],
      [{ did, name }],
    );
  } catch (e) {
    console.error(`adminbot: announcing bot ${did} failed: ${(e as Error).message}`);
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

// True iff `senderDid` is a current member of the #admins group. Privileged
// commands gate on this — the server trusts adminbot's session unconditionally
// (the superuser pin), so authorization for privileged chat commands must be
// enforced here, by checking the E2E #admins membership adminbot can decrypt.
// Fetches fresh group state so a just-removed admin can't keep issuing commands.
async function requireAdminsMember(
  core: AppCore,
  adminsGroupId: string,
  senderDid: string,
): Promise<boolean> {
  try {
    const summary = await core.fetchGroupState(adminsGroupId);
    return summary.members.some((m) => m.did === senderDid);
  } catch (e) {
    console.error(`adminbot: #admins membership check failed: ${(e as Error).message}`);
    return false;
  }
}

// ── /install-project interview ──────────────────────────────────────────────
//
// Installing a project is initiation + authorization, not data entry (see the
// bot-tool-ux skill). The project describes itself in a small manifest — its
// codename, name, and the permissions it wants. The operator hands over that
// manifest (pasted, or a URL adminbot fetches) and authorizes which permissions
// to grant; adminbot creates the project, grants them, and returns a one-time
// setup code for the project's bot.
//
// adminbot reacts 👀 on the trigger while the DM interview runs, then ✅ on
// success or ❌ on failure/cancel/timeout. State is in-memory and per-sender; a
// restart drops it (the operator re-runs). The manifest shape here mirrors the
// future well-known-URL manifest contract (docs TODO).

const REACT_WORKING = "👀";
const REACT_DONE = "✅";
const REACT_FAILED = "❌";

// Abandon an interview after this much operator inactivity.
const INTERVIEW_IDLE_MS = 10 * 60 * 1000;

// Plain-language descriptions for the permissions a manifest can request. The
// confirm step shows these, never the raw capability ids.
const PERMISSION_LABELS: Record<string, string> = {
  "accounts.read": "see the list of accounts on the server, and be told as people join or leave",
  "registration.gatekeeper": "control who is allowed to register",
};
const permissionLabel = (id: string): string => PERMISSION_LABELS[id] ?? id;

// A project's self-description. Mirrors the future well-known-URL manifest.
interface ProjectManifest {
  slug: string;
  name: string;
  description?: string;
  url?: string;
  permissions: string[];
}

type InstallStep = "manifest" | "confirm";

interface InstallInterview {
  step: InstallStep;
  manifest?: ProjectManifest; // set once a valid manifest is provided
  // The trigger message + where it lives, so we can react on it. `sentAt` is its
  // wire identity; null when we have no timestamp to target (reactions skipped).
  react: { target: SendTarget; author: string; sentAt: DecryptedMessage["sentAt"] } | null;
  timer: ReturnType<typeof setTimeout> | null;
}

// Keyed by the interviewee's DID — one interview at a time per admin.
const installInterviews = new Map<string, InstallInterview>();

// Set adminbot's reaction on the trigger message (a fresh emoji from the same
// reactor replaces the prior one, docs/33). No-op without a timestamp to target;
// failures are logged, never fatal.
async function setInterviewReaction(
  core: AppCore,
  react: InstallInterview["react"],
  emoji: string,
): Promise<void> {
  if (!react || !react.sentAt) return;
  try {
    await core.sendReaction(react.target, react.author, react.sentAt, emoji, false);
  } catch (e) {
    console.error(`adminbot: reaction failed: ${(e as Error).message}`);
  }
}

// Clear the idle timer (if any) and forget the interview.
function endInterview(senderDid: string): void {
  const iv = installInterviews.get(senderDid);
  if (iv?.timer) clearTimeout(iv.timer);
  installInterviews.delete(senderDid);
}

// (Re)arm the idle timeout after each interaction. On expiry: drop the state,
// mark the trigger failed, and tell the operator they can re-run.
function armIdleTimeout(core: AppCore, senderDid: string): void {
  const iv = installInterviews.get(senderDid);
  if (!iv) return;
  if (iv.timer) clearTimeout(iv.timer);
  const timer = setTimeout(() => {
    const stale = installInterviews.get(senderDid);
    installInterviews.delete(senderDid);
    if (stale) void setInterviewReaction(core, stale.react, REACT_FAILED);
    void core
      .sendDm(senderDid, "Timed out — nothing was installed. Run /install-project to start over.")
      .catch(() => {});
  }, INTERVIEW_IDLE_MS);
  timer.unref?.();
  iv.timer = timer;
}

// Parse a pasted manifest, or fetch one from a URL. Throws an Error with a
// plain-language message on any problem.
async function loadManifest(input: string): Promise<ProjectManifest> {
  let raw: string;
  if (/^https?:\/\//.test(input)) {
    const resp = await fetch(input).catch((e) => {
      throw new Error(`couldn't reach that address (${(e as Error).message}).`);
    });
    if (!resp.ok) throw new Error(`that address returned HTTP ${resp.status}.`);
    raw = await resp.text();
  } else {
    raw = input;
  }

  let obj: unknown;
  try {
    obj = JSON.parse(raw);
  } catch {
    throw new Error("that isn't a web address or valid manifest JSON.");
  }
  if (typeof obj !== "object" || obj === null) {
    throw new Error("the manifest must be a JSON object.");
  }
  const m = obj as Record<string, unknown>;

  if (typeof m.slug !== "string" || !/^[a-z0-9-]{2,64}$/.test(m.slug)) {
    throw new Error('the manifest needs a "slug" of 2–64 lowercase letters, numbers, or dashes.');
  }
  if (typeof m.name !== "string" || m.name.length === 0 || m.name.length > 100) {
    throw new Error('the manifest needs a "name" (1–100 characters).');
  }
  const url = typeof m.url === "string" ? m.url : undefined;
  if (url !== undefined && !/^https?:\/\//.test(url)) {
    throw new Error('the manifest\'s "url" must start with http:// or https://.');
  }
  let permissions: string[] = [];
  if (Array.isArray(m.permissions)) {
    if (!m.permissions.every((p): p is string => typeof p === "string")) {
      throw new Error('"permissions" must be a list of text values.');
    }
    permissions = m.permissions;
  } else if (m.permissions !== undefined) {
    throw new Error('"permissions" must be a list of text values.');
  }
  return {
    slug: m.slug,
    name: m.name,
    description: typeof m.description === "string" ? m.description : undefined,
    url,
    permissions,
  };
}

// The confirm prompt: what the project is + the permissions it requests,
// numbered, in plain language.
function confirmPrompt(m: ProjectManifest): string {
  const lines = [`${m.name} wants to be installed.`];
  if (m.description) lines.push(m.description);
  if (m.permissions.length === 0) {
    lines.push("", "It isn't requesting any special permissions.", "", "Reply `yes` to install, or /cancel.");
  } else {
    lines.push("", "It's asking permission to:");
    m.permissions.forEach((p, i) => lines.push(`  ${i + 1}. ${permissionLabel(p)}`));
    lines.push(
      "",
      'Reply `yes` to grant all, a list of numbers (e.g. "1") to grant some,',
      "`none` to grant nothing, or /cancel.",
    );
  }
  return lines.join("\n");
}

// Start the interview: react 👀 on the trigger and DM for the manifest. The
// trigger may be in #admins or a DM; either way the Q&A runs in a DM (the
// reaction is the in-channel "on it — check your DMs" signal).
async function startInstallInterview(
  core: AppCore,
  senderDid: string,
  channel: SendTarget,
  triggerSentAt: DecryptedMessage["sentAt"],
): Promise<void> {
  const react = { target: channel, author: senderDid, sentAt: triggerSentAt };
  installInterviews.set(senderDid, { step: "manifest", react, timer: null });
  await setInterviewReaction(core, react, REACT_WORKING);
  await core.sendDm(
    senderDid,
    "Let's install a project. Paste its manifest, or the web address I can fetch it from. (Reply /cancel anytime.)",
  );
  armIdleTimeout(core, senderDid);
}

// Consume one DM line as the answer to the current interview step.
async function handleInstallReply(
  core: AppCore,
  env: Env,
  senderDid: string,
  body: string,
): Promise<void> {
  const iv = installInterviews.get(senderDid);
  if (!iv) return;

  if (body.trim().toLowerCase() === "/cancel") {
    endInterview(senderDid);
    await setInterviewReaction(core, iv.react, REACT_FAILED);
    await core.sendDm(senderDid, "Install cancelled.");
    return;
  }

  if (iv.step === "manifest") {
    let manifest: ProjectManifest;
    try {
      manifest = await loadManifest(body.trim());
    } catch (e) {
      await core.sendDm(senderDid, `I couldn't read that — ${(e as Error).message} Try again, or /cancel.`);
      armIdleTimeout(core, senderDid);
      return;
    }
    // registration.gatekeeper needs a signing key this flow can't supply; drop
    // it from the request with a note (docs/24).
    const gated = manifest.permissions.includes("registration.gatekeeper");
    manifest.permissions = manifest.permissions.filter((p) => p !== "registration.gatekeeper");
    iv.manifest = manifest;
    iv.step = "confirm";
    if (gated) {
      await core.sendDm(
        senderDid,
        "Note: this project asks to control who can register — I can't grant that here (it needs a signing key), so I'll leave it out.",
      );
    }
    await core.sendDm(senderDid, confirmPrompt(manifest));
    armIdleTimeout(core, senderDid);
    return;
  }

  // step === "confirm": authorize which requested permissions to grant.
  const manifest = iv.manifest!;
  const reply = body.trim().toLowerCase();
  let grant: string[];
  if (reply === "yes" || reply === "y") {
    grant = manifest.permissions;
  } else if (reply === "none" || reply === "no") {
    grant = [];
  } else {
    const picked: string[] = [];
    for (const tok of body.trim().split(/[\s,]+/).filter((s) => s.length > 0)) {
      const i = Number(tok);
      if (!Number.isInteger(i) || i < 1 || i > manifest.permissions.length) {
        await core.sendDm(senderDid, "Reply `yes`, `none`, a list of numbers, or /cancel.");
        armIdleTimeout(core, senderDid);
        return;
      }
      picked.push(manifest.permissions[i - 1]);
    }
    grant = picked;
  }

  endInterview(senderDid);
  const result = await performInstall(core, env, manifest, grant);
  await core.sendDm(senderDid, result.lines.join("\n"));
  await setInterviewReaction(core, iv.react, result.ok ? REACT_DONE : REACT_FAILED);
}

// Create the project, grant the approved permissions, and mint the setup code.
// A pre-existing slug (409) is non-fatal — still (re)grant + re-issue the code.
async function performInstall(
  core: AppCore,
  env: Env,
  manifest: ProjectManifest,
  grant: string[],
): Promise<{ ok: boolean; lines: string[] }> {
  const { slug, name, url } = manifest;
  const lines: string[] = [];
  try {
    await core.adminRequest(
      "POST",
      "/v1/admin/projects",
      JSON.stringify({ slug, name, url: url ?? null, bot_dids: [] }),
    );
    lines.push(`Installed "${name}" (codename ${slug}).`);
  } catch (e) {
    const msg = (e as Error).message;
    if (msg.includes("409")) {
      lines.push(`"${name}" (codename ${slug}) already existed — updated it.`);
    } else {
      return { ok: false, lines: [`Install failed: ${msg}`] };
    }
  }

  const granted: string[] = [];
  const failures: string[] = [];
  for (const cap of grant) {
    try {
      await core.adminRequest(
        "POST",
        "/v1/admin/capabilities",
        JSON.stringify({ project_slug: slug, capability: cap }),
      );
      granted.push(cap);
    } catch (e) {
      failures.push(`${permissionLabel(cap)} (${(e as Error).message})`);
    }
  }
  if (granted.length) lines.push(`Granted: ${granted.map(permissionLabel).join(", ")}.`);
  if (failures.length) lines.push(`Couldn't grant: ${failures.join("; ")}.`);

  if (env.sharedSecret) {
    const token = AppCore.bootstrapToken(env.serverUrl, env.sharedSecret, slug);
    lines.push(
      "",
      "Setup code for the project's bot (sensitive — don't share; rotate",
      "REGISTRATION_SHARED_SECRET to revoke). Paste it into the bot's config as its",
      "invite token and it'll sign up and link to this project automatically:",
      token,
    );
  } else {
    lines.push(
      "",
      "No setup secret is configured here, so I can't issue a setup code. On a",
      "closed-registration server, set REGISTRATION_SHARED_SECRET; on an open server",
      "the bot can sign up without one but won't auto-link to this project.",
    );
  }
  return { ok: true, lines };
}

// Server view of an installed Project (superuser `GET /v1/admin/projects`).
// `bot_dids` is the set of bot accounts linked to the project — the server adds
// a bot's DID here when it registers with that project's setup code.
interface ProjectView {
  slug: string;
  name: string;
  url: string | null;
  has_signing_key: boolean;
  capabilities: string[];
  bot_dids: string[];
  superuser: boolean;
}

// Fetch installed Projects via the superuser admin API. Throws on transport or
// unexpected-response failure — callers decide how to degrade.
async function fetchProjects(core: AppCore): Promise<ProjectView[]> {
  const raw = await core.adminRequest("GET", "/v1/admin/projects", "");
  const parsed = JSON.parse(raw) as { projects?: ProjectView[] };
  if (!Array.isArray(parsed.projects)) throw new Error("unexpected server response");
  return parsed.projects;
}

// `/list-projects` — list installed Projects and their capabilities.
async function runListProjects(core: AppCore, channel: SendTarget): Promise<void> {
  let projects: ProjectView[];
  try {
    projects = await fetchProjects(core);
  } catch (e) {
    await core.send(channel, `list failed: ${(e as Error).message}`);
    return;
  }
  if (projects.length === 0) {
    await core.send(channel, "No projects installed.");
    return;
  }
  const lines = [`Installed projects (${projects.length}):`];
  for (const p of projects) {
    const tags = [
      p.superuser ? "superuser" : null,
      p.capabilities.length > 0 ? `caps: ${p.capabilities.join(", ")}` : "no caps",
      `bots: ${p.bot_dids.length}`,
    ].filter(Boolean);
    lines.push(`• ${p.name} (${p.slug})${p.url ? ` — ${p.url}` : ""} | ${tags.join(" | ")}`);
  }
  await core.send(channel, lines.join("\n"));
}

async function handleCommand(
  core: AppCore,
  env: Env,
  adminsGroupId: string,
  channel: SendTarget,
  senderDid: string,
  body: string,
  triggerSentAt: DecryptedMessage["sentAt"],
): Promise<void> {
  if (!body.startsWith("/")) return;
  const tokens = body.split(/\s+/);
  const cmd = tokens[0];
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
    case "/install-project":
      if (!(await requireAdminsMember(core, adminsGroupId, senderDid))) {
        await core.send(channel, "Only #admins members can install projects.");
        break;
      }
      // Parameters are gathered via a DM interview, not the command line.
      await startInstallInterview(core, senderDid, channel, triggerSentAt);
      break;
    case "/list-projects":
      if (!(await requireAdminsMember(core, adminsGroupId, senderDid))) {
        await core.send(channel, "Only #admins members can list projects.");
        break;
      }
      await runListProjects(core, channel);
      break;
    case "/help":
      await core.send(
        channel,
        [
          "Available commands:",
          "  /whoami           echo your DID",
          "  /audit            get status of all groups",
          "  /check            check now for a newer Avalanche release",
          "  /install-project  install a Project — I'll DM you to paste its manifest (admins only)",
          "  /list-projects    list installed Projects (admins only)",
          "  /help             show this help",
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
      handleAdminEvent(core, event, groupId).catch((e) => {
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
