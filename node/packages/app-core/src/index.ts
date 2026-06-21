/**
 * High-level TypeScript wrapper over the `app-core` napi-rs bindings.
 *
 * What this layer adds on top of the raw native module:
 *
 * - Millisecond `number` timestamps become `Temporal.Instant`.
 * - `deliveryStatus: number` becomes {@link DeliveryStatus} (`"sending" |
 *   "sent" | "delivered" | "read"`).
 * - Group `role: number` becomes {@link GroupRole} (`"member" | "admin"`).
 * - `Buffer` becomes `Uint8Array` on every byte field (zero-copy — `Buffer`
 *   is a `Uint8Array` subclass).
 * - {@link ConnectionState} and {@link IncomingEvent} are typed as
 *   discriminated unions.
 *
 * ### Runtime requirements
 *
 * Node ≥ 26 with a native `Temporal` global. Standard nodejs.org Node 26
 * binaries include it; some distro builds (notably `node:26-alpine` without
 * the Rust toolchain at build time) ship without it. If `globalThis.Temporal`
 * is undefined, any method that hands in or returns an `Instant` will throw.
 *
 * ### Getting started
 *
 * ```ts
 * import { initLogging, AppCore } from "@actnet/app-core";
 *
 * initLogging("info");
 *
 * const core = await AppCore.createAccount(
 *   "https://homeserver.example",
 *   "/var/lib/mybot/store.db",
 *   "", // SQLCipher passphrase
 *   new Uint8Array(0),     // no recovery key
 *   "MyBot",
 * );
 *
 * console.log("registered as", core.did());
 *
 * // Background receive loop.
 * (async () => {
 *   for await (const e of core.events()) {
 *     if (e.kind === "message") console.log("got:", e.message.body);
 *   }
 * })();
 *
 * await core.sendDm("did:plc:abc...", "hi");
 * ```
 *
 * @packageDocumentation
 */

import "temporal-spec/global";

import * as native from "../native/index.js";

// ── Public type aliases ─────────────────────────────────────────────────────

/**
 * Delivery status for a sent message.
 *
 * - `"sending"` — locally queued, not yet acknowledged by the server.
 * - `"sent"` — accepted by the server.
 * - `"delivered"` — recipient's device pulled it from the server.
 * - `"read"` — recipient sent a read receipt for it.
 *
 * @category Types
 */
export type DeliveryStatus = "sending" | "sent" | "delivered" | "read";

/**
 * Role of a member in a group.
 *
 * @category Types
 */
export type GroupRole = "member" | "admin";

/**
 * Where a plain-text {@link AppCore.send} is directed: a DM with a peer, or a
 * group. Discriminated by `kind`. Lets a caller route a reply back through
 * whichever channel a message arrived on without branching on the transport.
 *
 * @category Types
 */
export type SendTarget =
  | { kind: "dm"; recipientDid: string }
  | { kind: "group"; groupId: string };

/**
 * Liveness of the connection to the homeserver. Discriminated by `state`.
 *
 * The reconnect task transitions:
 * `"disconnected"` → `"connecting"` → `"connected"`, and on failure
 * → `"reconnecting"` (carries `nextAttemptAt` for backoff display).
 *
 * @category Types
 */
export type ConnectionState =
  | { state: "disconnected" }
  | { state: "connecting" }
  | { state: "connected" }
  | {
      state: "reconnecting";
      /** Instant of the next scheduled reconnect attempt. */
      nextAttemptAt: Temporal.Instant;
    };

/**
 * A single event surfaced by {@link AppCore.nextEvents}. Discriminated by
 * `kind`.
 *
 * @category Types
 */
export type IncomingEvent =
  | { kind: "message"; message: DecryptedMessage }
  | { kind: "receipt"; receipt: DeliveryStatusUpdate }
  | { kind: "groupInvite"; groupInvite: GroupInvite }
  | { kind: "groupMetadataChanged"; groupMetadata: GroupMetadataChanged };

/**
 * A `GroupContext` DM was received for `groupId` — the master key has been
 * persisted by app-core, so the group is now visible via
 * {@link AppCore.loadConversations}. Use this event to refresh the chat list.
 *
 * @category Types
 */
export interface GroupInvite {
  groupId: string;
  hostingServerUrl: string;
  inviterDid: string;
}

/**
 * A membership/metadata change derived from a group's change log (docs/03
 * §3.6) — e.g. someone joined, left, was added/removed, or a role changed.
 * app-core has already persisted the matching system row; bots can act on the
 * structured fields (e.g. adminbot noting it was added to a group) or log
 * {@link summary} directly.
 *
 * @category Types
 */
export interface GroupMetadataChanged {
  groupId: string;
  revision: number;
  /** e.g. `"memberJoined"`, `"memberRemoved"`, `"roleChangedToAdmin"`. */
  kind: string;
  /** DID of the actor who performed the change (empty if unattributed). */
  actorDid: string;
  /** DID of the member the change is about (empty if not yet known). */
  targetDid: string;
  /** base64 encrypted_member_id of the target, when the action names one. */
  targetEmi: string;
  occurredAt: Temporal.Instant;
  /** Ready-to-log English one-liner (uses DIDs, not display names). */
  summary: string;
}

/**
 * Admin-only events surfaced via {@link AppCore.adminEvents} /
 * {@link AppCore.nextAdminEvents}. Only adminbot sessions ever receive
 * these — for any other session the queue stays empty.
 *
 * Discriminated by `kind`; currently only `"accountJoined"`, but kept
 * extensible for future admin pushes.
 *
 * @category Types
 */
export type AdminEvent = { kind: "accountJoined"; accountJoined: AccountJoined };

/**
 * Adminbot-only event: a new account just registered on this homeserver.
 * Pushed only to bot sessions whose authed DID matches the server's
 * pinned `ADMINBOT_DID`. If this session isn't adminbot, this event
 * never fires.
 *
 * @category Types
 */
export interface AccountJoined {
  /** DID of the newly-registered account. */
  did: string;
  /** Server-side timestamp at the moment of registration. */
  joinedAt: Temporal.Instant;
}

/**
 * Public metadata about a Project (server-side bot or webview tool)
 * installed on the homeserver.
 *
 * @category Types
 */
export interface ProjectInfo {
  /** Human-readable name shown in the Projects list. */
  name: string;
  /** Project endpoint URL (used as the audience for project tokens). */
  url: string;
  /** Short description shown in the Projects list. */
  description: string;
}

/**
 * A decrypted inbound message received from the homeserver.
 *
 * @category Types
 */
export interface DecryptedMessage {
  /** Server-assigned monotonic id. Used for acking. */
  serverId: number;
  /** DID of the sender (`did:plc:...` or `did:local:...`). */
  senderDid: string;
  /** Sender's per-account device id. */
  senderDeviceId: number;
  /**
   * Decoded text body — UTF-8 lossily decoded from {@link plaintext}. The
   * default field to read for normal bots. For text DMs this is exactly
   * what the sender typed.
   */
  body: string;
  /**
   * Raw decrypted bytes. Same content as {@link body} for text DMs, but
   * may carry arbitrary binary payloads for group messages a future
   * application encodes itself. Use this if you need byte-exact data.
   */
  plaintext: Uint8Array;
  /** Sender's send-time (from envelope). Absent on legacy messages. */
  sentAt?: Temporal.Instant;
  /**
   * URL-safe-no-pad base64 of the group id when the message arrived as a
   * group message. Absent for plain DMs.
   */
  groupId?: string;
  /**
   * The sender's profile key carried on the envelope, if any. app-core does
   * NOT fetch or cache the sender's display name automatically — pass this to
   * {@link AppCore.fetchAndCacheProfile} if your bot wants the name. Absent
   * when the envelope carried no key. Most bots ignore this.
   */
  profileKey?: Uint8Array;
  /**
   * True when this is an inbound DM that the message-request gate treats as a
   * request (the sender is not curated and is not a known bot). app-core does
   * NOT persist a pending-request flag itself; a bot that tracks requests
   * calls {@link AppCore.setPendingRequest}. Always false for group messages.
   * Most bots ignore this.
   */
  isRequest: boolean;
}

/**
 * A message from local history (persisted in SQLCipher). Returned by
 * {@link AppCore.loadMessages}, {@link AppCore.loadLastMessage}, and
 * {@link AppCore.loadConversations}. Pass back to {@link AppCore.saveMessage}
 * to insert/update.
 *
 * @category Types
 */
export interface StoredMessage {
  /** Client-chosen id (typically a UUID). Primary key in the local store. */
  id: string;
  /**
   * Conversation key. For DMs this is the other party's DID; for group
   * messages this is the group id.
   */
  conversationId: string;
  /** DID of the sender. */
  senderDid: string;
  /** Plaintext body. */
  body: string;
  /** Sender's send-time. */
  sentAt: Temporal.Instant;
  /** When the message was last edited, if ever. */
  editedAt?: Temporal.Instant;
  /** When this row was marked read locally, if ever. */
  readAt?: Temporal.Instant;
  /** Outbound delivery status (incoming messages stay at `"delivered"`). */
  deliveryStatus: DeliveryStatus;
  /**
   * 0 = normal chat message; >0 = a system/metadata timeline entry (docs/03
   * §3.6 group event). Renderers show kind>0 rows as a centered line.
   */
  kind: number;
  /**
   * JSON for system rows (event kind + actor/target DIDs); `undefined` for
   * normal chat messages.
   */
  metadata?: string;
}

/**
 * One row per conversation that has at least one persisted message, paired
 * with that conversation's most recent message. The chat list is rendered
 * directly from this.
 *
 * @category Types
 */
export interface ConversationSummary {
  /** Same key as {@link StoredMessage.conversationId}. */
  conversationId: string;
  /** Most recent message in the conversation. `undefined` for conversations
   *  we know about (e.g. groups we've been invited to) that don't yet have
   *  any persisted messages. */
  lastMessage?: StoredMessage;
}

/**
 * A delivery-status change for an outgoing message (e.g. a read receipt
 * arrived). Surfaced via {@link AppCore.nextEvents}.
 *
 * @category Types
 */
export interface DeliveryStatusUpdate {
  /** Conversation the affected message belongs to. */
  conversationId: string;
  /** `sentAt` of the affected message, used to look it up locally. */
  sentAt: Temporal.Instant;
  /** New status. */
  deliveryStatus: DeliveryStatus;
}

/**
 * Result of {@link AppCore.createGroup}.
 *
 * @category Types
 */
export interface CreatedGroup {
  /** URL-safe-no-pad base64 group id. Use everywhere the API takes `groupId`. */
  groupId: string;
  /**
   * 32-byte zkgroup master key. Stash it; it's the secret an invite link
   * carries, and any future device-recovery flow needs it back.
   */
  masterKey: Uint8Array;
}

/**
 * A member's decrypted row in a group.
 *
 * @category Types
 */
export interface GroupMember {
  /** Member's DID. */
  did: string;
  /**
   * URL-safe-no-pad base64 of the encrypted member id. Pass this verbatim
   * to admin actions ({@link AppCore.removeMember},
   * {@link AppCore.changeMemberRole}, etc.).
   */
  encryptedMemberId: string;
  /** Member or admin. */
  role: GroupRole;
  /** When this member joined. */
  joinedAt: Temporal.Instant;
}

/**
 * Pending-invite or pending-approval entry in a group.
 *
 * @category Types
 */
export interface GroupPending {
  /** Server-visible encrypted member id. */
  encryptedMemberId: string;
  /**
   * For `pendingInvites` this is `invitedAt`. For `pendingApprovals` it's
   * `requestedAt`.
   */
  at: Temporal.Instant;
}

/**
 * Snapshot of a group's decrypted state. Returned by
 * {@link AppCore.fetchGroupState}.
 *
 * @category Types
 */
export interface GroupSummary {
  /** URL-safe-no-pad base64 group id. */
  groupId: string;
  /** 32-byte zkgroup master key. */
  masterKey: Uint8Array;
  /** Monotonic revision. Bumps on every membership change. */
  revision: number;
  title: string;
  description: string;
  /** Disappearing-messages timer, in seconds. `0` means off. */
  expirySeconds: number;
  members: GroupMember[];
  pendingInvites: GroupPending[];
  pendingApprovals: GroupPending[];
}

/**
 * Minimal contact-list row. Backs the compose autocomplete and the People
 * list.
 *
 * @category Types
 */
export interface ContactRow {
  did: string;
  /** Cached profile display name. Empty string if not fetched yet. */
  displayName: string;
  /**
   * `true` if the user has done a deliberate gesture toward this contact
   * (sent them a DM, invited them to a group). The compose autocomplete
   * shows curated rows under "People" and the rest under "Other".
   */
  isCurated: boolean;
  /** Most recent interaction with this contact. */
  lastInteractionAt: Temporal.Instant;
}

/**
 * Public metadata returned by {@link AppCore.getAccountInfo}. Server-side
 * lookup that does not require any prior interaction with the account.
 *
 * @category Types
 */
export interface AccountInfo {
  did: string;
  /** Only populated for bot accounts. Human names live in encrypted profiles. */
  displayName?: string;
  isBot: boolean;
}

/**
 * Decoded invite-token info returned by {@link validateInvite}. Shown on
 * the invite acceptance screen before any further server communication.
 *
 * @category Types
 */
export interface InviteInfo {
  /** Homeserver URL the new account should register against. */
  serverUrl: string;
  /** Human-readable server name (from `/v1/server-info`). */
  serverName: string;
  /** Inviter's DID, if present in the token. */
  inviterDid?: string;
  /** Where to send the user after onboarding, if the token specifies. */
  postOnboardingRedirect?: string;
  /** Inviter's plaintext display name from the token. */
  inviterDisplayName?: string;
  /** Inviter's 32-byte profile key (used to prime the contact-profile cache). */
  inviterProfileKey?: Uint8Array;
}

/**
 * Outcome of {@link AppCore.joinViaLink}.
 *
 * - `"member"` — admitted directly (open-link group).
 * - `"pending"` — placed in the approval queue; admins must approve before
 *   the caller can act.
 *
 * @category Types
 */
export type JoinResult = "member" | "pending";

// ── Converters (internal) ───────────────────────────────────────────────────

// Buffer is a Uint8Array subclass; the cast is a no-op at runtime.
const asU8 = (b: Uint8Array): Uint8Array => b;
const asBuf = (u: Uint8Array): Buffer => (Buffer.isBuffer(u) ? (u as Buffer) : Buffer.from(u));

// Reused across every receive — lossy decode matches the Rust side's
// `String::from_utf8_lossy` on send.
const utf8Decoder = new TextDecoder("utf-8", { fatal: false });
const decodeBody = (bytes: Uint8Array): string => utf8Decoder.decode(bytes);
const encodeBody = (body: string): Buffer => Buffer.from(body, "utf8");

const DELIVERY: DeliveryStatus[] = ["sending", "sent", "delivered", "read"];
const deliveryFromNum = (n: number): DeliveryStatus => {
  const s = DELIVERY[n];
  if (!s) throw new RangeError(`unknown delivery status: ${n}`);
  return s;
};
const deliveryToNum = (s: DeliveryStatus): number => DELIVERY.indexOf(s);

const roleFromNum = (n: number): GroupRole => (n === 1 ? "admin" : "member");
const roleToNum = (r: GroupRole): number => (r === "admin" ? 1 : 0);

const instantFromMs = (ms: number): Temporal.Instant =>
  Temporal.Instant.fromEpochMilliseconds(ms);
const instantToMs = (i: Temporal.Instant): number => Number(i.epochMilliseconds);
const instantFromMsOpt = (ms: number | null | undefined): Temporal.Instant | undefined =>
  ms == null ? undefined : instantFromMs(ms);

const connStateFromNative = (s: native.ConnectionStateJs): ConnectionState => {
  switch (s.state) {
    case "disconnected": return { state: "disconnected" };
    case "connecting": return { state: "connecting" };
    case "connected": return { state: "connected" };
    case "reconnecting":
      return { state: "reconnecting", nextAttemptAt: instantFromMs(s.nextAttemptAtMs ?? 0) };
    default:
      throw new Error(`unknown connection state: ${s.state}`);
  }
};
const connStateToNative = (s: ConnectionState): native.ConnectionStateJs => {
  if (s.state === "reconnecting") {
    return { state: "reconnecting", nextAttemptAtMs: instantToMs(s.nextAttemptAt) };
  }
  return { state: s.state };
};

const sendTargetToNative = (t: SendTarget): native.MessageTargetJs =>
  t.kind === "dm"
    ? { kind: "dm", recipientDid: t.recipientDid }
    : { kind: "group", groupId: t.groupId };

const decryptedMessageFromNative = (m: native.DecryptedMessageJs): DecryptedMessage => {
  const plaintext = asU8(m.plaintext);
  return {
    serverId: m.serverId,
    senderDid: m.senderDid,
    senderDeviceId: m.senderDeviceId,
    body: decodeBody(plaintext),
    plaintext,
    sentAt: instantFromMsOpt(m.sentAtMs),
    groupId: m.groupId,
    profileKey: m.profileKey ? asU8(m.profileKey) : undefined,
    isRequest: m.isRequest,
  };
};

const storedMessageFromNative = (m: native.StoredMessageJs): StoredMessage => ({
  id: m.id,
  conversationId: m.conversationId,
  senderDid: m.senderDid,
  body: m.body,
  sentAt: instantFromMs(m.sentAtMs),
  editedAt: instantFromMsOpt(m.editedAtMs),
  readAt: instantFromMsOpt(m.readAtMs),
  deliveryStatus: deliveryFromNum(m.deliveryStatus),
  kind: m.kind,
  metadata: m.metadata ?? undefined,
});

const storedMessageToNative = (m: StoredMessage): native.StoredMessageJs => ({
  id: m.id,
  conversationId: m.conversationId,
  senderDid: m.senderDid,
  body: m.body,
  sentAtMs: instantToMs(m.sentAt),
  editedAtMs: m.editedAt ? instantToMs(m.editedAt) : undefined,
  readAtMs: m.readAt ? instantToMs(m.readAt) : undefined,
  deliveryStatus: deliveryToNum(m.deliveryStatus),
  // The JS layer only saves normal chat messages; app-core writes system rows.
  kind: m.kind ?? 0,
  metadata: m.metadata ?? undefined,
});

const conversationSummaryFromNative = (c: native.ConversationSummaryJs): ConversationSummary => ({
  conversationId: c.conversationId,
  lastMessage: c.lastMessage ? storedMessageFromNative(c.lastMessage) : undefined,
});

const deliveryStatusUpdateFromNative = (u: native.DeliveryStatusUpdateJs): DeliveryStatusUpdate => ({
  conversationId: u.conversationId,
  sentAt: instantFromMs(u.sentAtMs),
  deliveryStatus: deliveryFromNum(u.deliveryStatus),
});

// Returns `null` for event kinds this SDK version doesn't surface (today:
// reactionUpdated / messageEdited / messageDeleted — docs/33, docs/36 — whose
// store side-effects are already applied in the core), and for any future kind
// added in Rust. Skipping rather than throwing keeps node consumers (adminbot,
// testbot) resilient: a new event variant must not crash a bot that ignores it.
const incomingEventFromNative = (e: native.IncomingEventJs): IncomingEvent | null => {
  if (e.kind === "message" && e.message) {
    return { kind: "message", message: decryptedMessageFromNative(e.message) };
  }
  if (e.kind === "receipt" && e.receipt) {
    return { kind: "receipt", receipt: deliveryStatusUpdateFromNative(e.receipt) };
  }
  if (e.kind === "groupInvite" && e.groupInvite) {
    return {
      kind: "groupInvite",
      groupInvite: {
        groupId: e.groupInvite.groupId,
        hostingServerUrl: e.groupInvite.hostingServerUrl,
        inviterDid: e.groupInvite.inviterDid,
      },
    };
  }
  if (e.kind === "groupMetadataChanged" && e.groupMetadata) {
    return {
      kind: "groupMetadataChanged",
      groupMetadata: {
        groupId: e.groupMetadata.groupId,
        revision: e.groupMetadata.revision,
        kind: e.groupMetadata.kind,
        actorDid: e.groupMetadata.actorDid,
        targetDid: e.groupMetadata.targetDid,
        targetEmi: e.groupMetadata.targetEmi,
        occurredAt: instantFromMs(e.groupMetadata.occurredAtMs),
        summary: e.groupMetadata.summary,
      },
    };
  }
  return null;
};

const adminEventFromNative = (e: native.AdminEventJs): AdminEvent => {
  if (e.kind === "accountJoined" && e.accountJoined) {
    return {
      kind: "accountJoined",
      accountJoined: {
        did: e.accountJoined.did,
        joinedAt: instantFromMs(e.accountJoined.joinedAtMs),
      },
    };
  }
  throw new Error(`malformed admin event: ${JSON.stringify(e)}`);
};

const groupMemberFromNative = (m: native.GroupMemberJs): GroupMember => ({
  did: m.did,
  encryptedMemberId: m.encryptedMemberId,
  role: roleFromNum(m.role),
  joinedAt: instantFromMs(m.joinedAtMs),
});

const groupPendingFromNative = (p: native.GroupPendingJs): GroupPending => ({
  encryptedMemberId: p.encryptedMemberId,
  at: instantFromMs(p.timestampMs),
});

const groupSummaryFromNative = (s: native.GroupSummaryJs): GroupSummary => ({
  groupId: s.groupId,
  masterKey: asU8(s.masterKey),
  revision: s.revision,
  title: s.title,
  description: s.description,
  expirySeconds: s.expirySeconds,
  members: s.members.map(groupMemberFromNative),
  pendingInvites: s.pendingInvites.map(groupPendingFromNative),
  pendingApprovals: s.pendingApprovals.map(groupPendingFromNative),
});

const createdGroupFromNative = (g: native.CreatedGroupJs): CreatedGroup => ({
  groupId: g.groupId,
  masterKey: asU8(g.masterKey),
});

const contactRowFromNative = (c: native.ContactRowJs): ContactRow => ({
  did: c.did,
  displayName: c.displayName,
  isCurated: c.isCurated,
  lastInteractionAt: instantFromMs(c.lastInteractionAtMs),
});

const accountInfoFromNative = (a: native.AccountInfoJs): AccountInfo => ({
  did: a.did,
  displayName: a.displayName,
  isBot: a.isBot,
});

const inviteInfoFromNative = (i: native.InviteInfoJs): InviteInfo => ({
  serverUrl: i.serverUrl,
  serverName: i.serverName,
  inviterDid: i.inviterDid,
  postOnboardingRedirect: i.postOnboardingRedirect,
  inviterDisplayName: i.inviterDisplayName,
  inviterProfileKey: i.inviterProfileKey ? asU8(i.inviterProfileKey) : undefined,
});

const joinResultFromNative = (r: native.JoinResultJs): JoinResult =>
  r === native.JoinResultJs.Member ? "member" : "pending";

// ── AppCore ─────────────────────────────────────────────────────────────────

/**
 * The main client handle. Wraps a SQLCipher-backed local store, a HTTP/WS
 * client for the homeserver, and a background reconnect task.
 *
 * Construct via one of the four static factories
 * ({@link AppCore.createAccount}, {@link AppCore.finalizeAccount},
 * {@link AppCore.recoverFromBlob}, {@link AppCore.login}). Each instance
 * spawns a background task that owns the WebSocket lifecycle and pushes
 * decrypted messages + delivery-status updates into the
 * {@link AppCore.nextEvents} queue.
 *
 * All methods are safe to call concurrently. Async methods run on the napi
 * libuv threadpool so they do not block the JS event loop.
 *
 * @category Client
 */
export class AppCore {
  /** @internal */ readonly _native: native.AppCore;

  /** @internal */ constructor(n: native.AppCore) {
    this._native = n;
  }

  // ── constructors ────────────────────────────────────────────────────────

  /**
   * Create a brand-new account on the homeserver.
   *
   * Generates an identity keypair + rotation key, computes a `did:plc:...`,
   * submits the PLC genesis op, registers with the homeserver, and (if
   * `recoveryKey` is non-empty) uploads an encrypted recovery blob.
   *
   * @param serverUrl   Homeserver URL.
   * @param dbPath      Where to create the SQLCipher database file.
   * @param dbKey       Passphrase used to derive the SQLCipher key.
   * @param recoveryKey 32-byte symmetric key (from passkey PRF or recovery
   *                    phrase). Pass an empty `Uint8Array` to skip recovery
   *                    setup.
   * @param displayName Profile display name. Encrypted under a fresh
   *                    profile key and uploaded with registration.
   *
   * @category Constructors
   */
  static async createAccount(
    serverUrl: string,
    dbPath: string,
    dbKey: string,
    recoveryKey: Uint8Array,
    displayName: string,
    inviteToken?: string,
  ): Promise<AppCore> {
    return new AppCore(
      await native.AppCore.createAccount(serverUrl, dbPath, dbKey, asBuf(recoveryKey), displayName, inviteToken),
    );
  }

  /**
   * Register a brand-new **bot** account on the homeserver.
   *
   * Bot accounts skip the PLC directory and receive a `did:local:...` DID
   * assigned by the server. `displayName` is stored plaintext on the server
   * (so bots can be looked up by name); humans use {@link createAccount}
   * which encrypts the display name into a profile blob instead.
   *
   * No recovery blob is uploaded — bots are operator-managed and don't use
   * the passkey recovery flow.
   *
   * @category Constructors
   */
  static async createBotAccount(
    serverUrl: string,
    dbPath: string,
    dbKey: string,
    displayName: string,
    didSuffix?: string,
    inviteToken?: string,
  ): Promise<AppCore> {
    return new AppCore(
      await native.AppCore.createBotAccount(
        serverUrl,
        dbPath,
        dbKey,
        displayName,
        didSuffix,
        inviteToken,
      ),
    );
  }

  /**
   * Finalize registration using a previously prepared identity.
   *
   * Use this when a passkey ceremony needs the DID up front: call
   * {@link PreparedAccount.create} to compute the DID locally, register the
   * passkey with that DID, then call this to encrypt the recovery blob,
   * submit the PLC genesis op, and complete server registration.
   *
   * The `prepared` handle is consumed.
   *
   * @category Constructors
   */
  static async finalizeAccount(
    prepared: PreparedAccount,
    dbPath: string,
    dbKey: string,
    displayName: string,
    inviteToken?: string,
  ): Promise<AppCore> {
    return new AppCore(
      await native.AppCore.finalizeAccount(
        prepared._native, dbPath, dbKey, displayName, inviteToken,
      ),
    );
  }

  /**
   * Recover an account from a passkey-protected recovery blob.
   *
   * Downloads the encrypted blob, decrypts it with `recoveryKey`, restores
   * the identity + rotation keys into a freshly opened SQLCipher store,
   * registers a replacement device with the homeserver, and returns an
   * {@link AppCore} ready to use.
   *
   * Contacts see no safety-number change because the original identity key
   * is preserved.
   *
   * @category Constructors
   */
  static async recoverFromBlob(
    serverUrl: string,
    did: string,
    recoveryKey: Uint8Array,
    dbPath: string,
    dbKey: string,
    displayName: string,
  ): Promise<AppCore> {
    return new AppCore(
      await native.AppCore.recoverFromBlob(
        serverUrl, did, asBuf(recoveryKey), dbPath, dbKey, displayName,
      ),
    );
  }

  /**
   * Open an existing account from a local SQLCipher store and authenticate
   * with the homeserver.
   *
   * @throws if the store has no account; if `dbKey` is wrong; or if the
   *         server rejects authentication.
   *
   * @category Constructors
   */
  static async login(dbPath: string, dbKey: string): Promise<AppCore> {
    return new AppCore(await native.AppCore.login(dbPath, dbKey));
  }

  /**
   * Open a bot account, registering it on first run and re-logging-in
   * thereafter — the idempotent bootstrap an operator-run bot needs.
   *
   * If the store at `dbPath` already holds an account, this logs in and
   * `serverUrl` / `displayName` / `didSuffix` are ignored (they were fixed by
   * the original registration). Otherwise — a fresh deploy, or an empty DB
   * left by a registration that failed after opening the store — it registers
   * a new {@link createBotAccount bot account}.
   *
   * The "is the store empty?" decision is made inside the core, so callers no
   * longer pattern-match on error strings to tell a first run from a real
   * failure. Layer identity policy on top by checking {@link AppCore.did}
   * against the DID you expect.
   *
   * @category Constructors
   */
  static async loginOrCreateBot(
    serverUrl: string,
    dbPath: string,
    dbKey: string,
    displayName: string,
    didSuffix?: string,
    inviteToken?: string,
  ): Promise<AppCore> {
    return new AppCore(
      await native.AppCore.loginOrCreateBot(serverUrl, dbPath, dbKey, displayName, didSuffix, inviteToken),
    );
  }

  /**
   * Build a bootstrap registration token from a shared secret (docs/24).
   *
   * Pass the result as `inviteToken` to {@link createBotAccount} /
   * {@link loginOrCreateBot} to register against a closed-registration server.
   * Naming `project` links the new account into that Project — e.g. the
   * superuser Project (slug `"adminbot"`) to bootstrap admin authority.
   *
   * @category Constructors
   */
  static bootstrapToken(serverUrl: string, secret: string, project?: string): string {
    // Single-char wire keys keep the token (and QR) compact: s=server_url,
    // k=bootstrap_secret, p=project (docs/24, 51).
    const payload: Record<string, string> = { s: serverUrl, k: secret };
    if (project) payload.p = project;
    return Buffer.from(JSON.stringify(payload), "utf8")
      .toString("base64")
      .replace(/=+$/, "").replace(/\+/g, "-").replace(/\//g, "_");
  }

  // ── identity ────────────────────────────────────────────────────────────

  /**
   * This account's DID (`did:plc:...` or `did:local:...`).
   *
   * @category Identity
   */
  did(): string {
    return this._native.did();
  }

  /**
   * This account's per-device id. Stable for the lifetime of the local store.
   *
   * @category Identity
   */
  deviceId(): number {
    return this._native.deviceId();
  }

  // ── messaging ───────────────────────────────────────────────────────────

  /**
   * Send an encrypted DM. The body is wrapped in a content envelope (with
   * `sentAt`) before encryption and fanned out to every active device of
   * the recipient.
   *
   * @param body   Text body. Sent verbatim — UTF-8 on the wire.
   * @param sentAt Send-time stamped into the envelope. Defaults to
   *               `Temporal.Now.instant()`.
   *
   * @category Direct Messages
   */
  async sendDm(recipientDid: string, body: string, sentAt?: Temporal.Instant): Promise<void> {
    const ts = sentAt ?? Temporal.Now.instant();
    await this._native.sendDm(recipientDid, encodeBody(body), instantToMs(ts));
  }

  /**
   * Send a text message to a {@link SendTarget} — a DM or a group — without
   * branching on the transport yourself. A `"dm"` target behaves exactly like
   * {@link sendDm} (envelope + per-device fan-out + marks the contact
   * curated); a `"group"` target behaves like {@link sendGroupMessage} (Sender
   * Key encryption + per-member fan-out).
   *
   * Handy for replying back through whichever channel a message arrived on:
   *
   * ```ts
   * const target: SendTarget = msg.groupId
   *   ? { kind: "group", groupId: msg.groupId }
   *   : { kind: "dm", recipientDid: msg.senderDid };
   * await core.send(target, "got it");
   * ```
   *
   * @param body   Text body. Sent verbatim — UTF-8 on the wire.
   * @param sentAt Send-time stamped into the envelope. Defaults to
   *               `Temporal.Now.instant()`.
   *
   * @category Direct Messages
   */
  async send(target: SendTarget, body: string, sentAt?: Temporal.Instant): Promise<void> {
    const ts = sentAt ?? Temporal.Now.instant();
    await this._native.sendMessage(sendTargetToNative(target), encodeBody(body), instantToMs(ts));
  }

  /**
   * React to a message in a DM or group (docs/33-reactions.md). One reaction
   * per account per message: a fresh `emoji` replaces any prior one; `remove`
   * clears it. Applies locally and sends a reaction control message to the
   * conversation.
   *
   * @param target       Where the reacted-to message lives (DM peer or group).
   * @param targetAuthor DID of the reacted-to message's author.
   * @param targetSentAt `sentAt` of the reacted-to message — its wire identity.
   * @param emoji        The reaction emoji (ignored when `remove` is `true`).
   * @param remove       `true` to clear this account's reaction on the target.
   * @param sentAt       This reaction op's timestamp. Defaults to now.
   *
   * @category Direct Messages
   */
  async sendReaction(
    target: SendTarget,
    targetAuthor: string,
    targetSentAt: Temporal.Instant,
    emoji: string,
    remove: boolean,
    sentAt?: Temporal.Instant,
  ): Promise<void> {
    const ts = sentAt ?? Temporal.Now.instant();
    await this._native.sendReaction(
      sendTargetToNative(target),
      targetAuthor,
      instantToMs(targetSentAt),
      emoji,
      remove,
      instantToMs(ts),
    );
  }

  /**
   * Fetch and decrypt all pending messages from the homeserver.
   *
   * Most callers should use {@link AppCore.nextEvents} (the push-driven
   * stream) instead. This is the explicit-pull variant, mainly useful for
   * tests and one-shot tools.
   *
   * @category Direct Messages
   */
  async receiveMessages(): Promise<DecryptedMessage[]> {
    const msgs = await this._native.receiveMessages();
    return msgs.map(decryptedMessageFromNative);
  }

  /**
   * Send a read receipt for a batch of messages to `recipientDid`. Fans
   * out across all of the recipient's active devices.
   *
   * @param sentAts `sentAt` instants of the messages being acknowledged.
   *
   * @category Direct Messages
   */
  async sendReadReceipt(recipientDid: string, sentAts: Temporal.Instant[]): Promise<void> {
    await this._native.sendReadReceipt(recipientDid, sentAts.map(instantToMs));
  }

  // ── connection ──────────────────────────────────────────────────────────

  /**
   * Snapshot of the current connection state. Non-blocking.
   *
   * @category Connection
   */
  connectionState(): ConnectionState {
    return connStateFromNative(this._native.connectionState());
  }

  /**
   * Block (off the event loop) until the connection state differs from
   * `last`, then return the new value. Typically used in a long-running
   * `while` loop to drive a UI indicator.
   *
   * @category Connection
   */
  async waitForConnectionStateChange(last: ConnectionState): Promise<ConnectionState> {
    const next = await this._native.waitForConnectionStateChange(connStateToNative(last));
    return connStateFromNative(next);
  }

  /**
   * Async iterator over decrypted messages and delivery-status updates from
   * the homeserver. The recommended receive path:
   *
   * ```ts
   * for await (const e of core.events()) {
   *   if (e.kind === "message") console.log(e.message.body);
   * }
   * ```
   *
   * Internally drains the native batch queue and yields one event at a
   * time. Single-consumer: run from exactly one async loop. The iterator
   * runs forever; `break` (or `return`) to stop, or it ends if the channel
   * is closed (i.e. the {@link AppCore} is torn down).
   *
   * @category Connection
   */
  async *events(): AsyncGenerator<IncomingEvent, void, void> {
    while (true) {
      let batch: native.IncomingEventJs[];
      try {
        batch = await this._native.nextEvents();
      } catch {
        return;
      }
      for (const e of batch) {
        const ev = incomingEventFromNative(e);
        if (ev) yield ev;
      }
    }
  }

  /**
   * Lower-level variant of {@link events}: block until at least one event
   * is available, then drain and return the whole batch.
   *
   * Prefer {@link events} for normal use; this is here for callers that
   * want explicit batch-processing semantics. Same single-consumer rule
   * applies. Throws when the event channel is closed.
   *
   * @category Connection
   */
  async nextEvents(): Promise<IncomingEvent[]> {
    const events = await this._native.nextEvents();
    return events
      .map(incomingEventFromNative)
      .filter((e): e is IncomingEvent => e !== null);
  }

  /**
   * Async iterator over admin-only events from the homeserver. Mirrors
   * {@link events} but for the parallel admin queue. Only adminbot sessions
   * ever receive admin events; for any other session this iterator hangs
   * forever waiting on an empty queue.
   *
   * ```ts
   * for await (const e of core.adminEvents()) {
   *   if (e.kind === "accountJoined") await invite(e.accountJoined.did);
   * }
   * ```
   *
   * Single-consumer: run from exactly one async loop. Drive it concurrently
   * with {@link events} via `Promise.all`.
   *
   * @category Connection
   */
  async *adminEvents(): AsyncGenerator<AdminEvent, void, void> {
    while (true) {
      let batch: native.AdminEventJs[];
      try {
        batch = await this._native.nextAdminEvents();
      } catch {
        return;
      }
      for (const e of batch) yield adminEventFromNative(e);
    }
  }

  /**
   * Lower-level variant of {@link adminEvents}: block until at least one
   * admin event is available, then drain and return the whole batch.
   *
   * @category Connection
   */
  async nextAdminEvents(): Promise<AdminEvent[]> {
    const events = await this._native.nextAdminEvents();
    return events.map(adminEventFromNative);
  }

  // ── projects ────────────────────────────────────────────────────────────

  /**
   * List the Projects installed on this homeserver.
   *
   * @category Projects
   */
  async fetchProjects(): Promise<ProjectInfo[]> {
    return await this._native.fetchProjects();
  }

  /**
   * Request a short-lived token for opening a Project's webview / API.
   *
   * @category Projects
   */
  async requestProjectToken(projectUrl: string): Promise<string> {
    return await this._native.requestProjectToken(projectUrl);
  }

  // ── local message history ───────────────────────────────────────────────

  /**
   * Insert or update a message in local history (SQLCipher).
   *
   * @category Local History
   */
  async saveMessage(msg: StoredMessage): Promise<void> {
    await this._native.saveMessage(storedMessageToNative(msg));
  }

  /**
   * Load all persisted messages for a conversation, oldest first.
   *
   * @category Local History
   */
  async loadMessages(conversationId: string): Promise<StoredMessage[]> {
    const msgs = await this._native.loadMessages(conversationId);
    return msgs.map(storedMessageFromNative);
  }

  /**
   * Enumerate every conversation with at least one persisted message,
   * along with its most recent message. Sorted newest-first.
   *
   * @category Local History
   */
  async loadConversations(): Promise<ConversationSummary[]> {
    const rows = await this._native.loadConversations();
    return rows.map(conversationSummaryFromNative);
  }

  /**
   * Most recent message for a conversation, or `null` if it has none.
   *
   * @category Local History
   */
  async loadLastMessage(conversationId: string): Promise<StoredMessage | null> {
    const msg = await this._native.loadLastMessage(conversationId);
    return msg ? storedMessageFromNative(msg) : null;
  }

  /**
   * Mark every message in the conversation with `sentAt ≤ upTo` as read.
   *
   * @returns Number of rows newly marked.
   *
   * @category Local History
   */
  async markMessagesRead(conversationId: string, upTo: Temporal.Instant): Promise<number> {
    return await this._native.markMessagesRead(conversationId, instantToMs(upTo));
  }

  /**
   * Count unread messages in a conversation.
   *
   * @category Local History
   */
  async unreadCount(conversationId: string): Promise<number> {
    return await this._native.unreadCount(conversationId);
  }

  // ── account / profile / contacts ────────────────────────────────────────

  /**
   * Fetch public metadata for any account.
   *
   * @category Account
   */
  async getAccountInfo(did: string): Promise<AccountInfo> {
    return accountInfoFromNative(await this._native.getAccountInfo(did));
  }

  /**
   * Register / refresh this device's push token with the relay and the
   * homeserver. Idempotent; safe (and recommended) to call on every launch.
   *
   * Rotates the pseudonym after ~7 days or when `(deviceToken, platform)`
   * changes.
   *
   * @param platform     `"apns"` (iOS) or `"fcm"` (Android).
   * @param environment  `"sandbox"` for debug builds, `"production"` for
   *                     App Store / TestFlight builds.
   *
   * @category Account
   */
  async registerPushToken(
    deviceToken: string,
    platform: "apns" | "fcm",
    relayUrl: string,
    environment: "sandbox" | "production",
  ): Promise<void> {
    await this._native.registerPushToken(deviceToken, platform, relayUrl, environment);
  }

  /**
   * Re-upload the encrypted recovery blob (for instance after joining a
   * new homeserver).
   *
   * @param recoveryKey 32-byte symmetric key.
   * @param servers     Updated list of homeserver URLs.
   *
   * @category Account
   */
  async updateRecoveryBlob(recoveryKey: Uint8Array, servers: string[]): Promise<void> {
    await this._native.updateRecoveryBlob(asBuf(recoveryKey), servers);
  }

  /**
   * Whether this account has a recovery blob configured on the server.
   *
   * @category Account
   */
  async hasRecovery(): Promise<boolean> {
    return await this._native.hasRecovery();
  }

  /**
   * This user's own display name from the local profile cache. Empty
   * string until a profile has been set.
   *
   * @category Profile
   */
  async ownDisplayName(): Promise<string> {
    return await this._native.ownDisplayName();
  }

  /**
   * Update the user's display name. Re-encrypts and uploads the profile
   * blob, then updates the local cache.
   *
   * @category Profile
   */
  async setDisplayName(displayName: string): Promise<void> {
    await this._native.setDisplayName(displayName);
  }

  /**
   * Cached display name for a contact. Empty string if no profile has been
   * fetched yet for this DID (caller should fall back to the DID).
   *
   * @category Profile
   */
  async contactDisplayName(did: string): Promise<string> {
    return await this._native.contactDisplayName(did);
  }

  /**
   * Re-fetch a contact's encrypted profile and update the local cache.
   *
   * @returns `true` if the cached display name changed.
   *
   * @category Profile
   */
  async refreshContactProfile(did: string): Promise<boolean> {
    return await this._native.refreshContactProfile(did);
  }

  /**
   * Prime the contact-profile cache with metadata extracted from an invite
   * token (so the auto-DM to the inviter shows their name from the first
   * frame). Call right after {@link AppCore.createAccount} when an invite
   * was accepted.
   *
   * @category Profile
   */
  async primeContactProfile(did: string, displayName: string, profileKey: Uint8Array): Promise<void> {
    await this._native.primeContactProfile(did, displayName, asBuf(profileKey));
  }

  /**
   * Every known contact, newest-interaction-first. Joins the curation flag
   * from `contacts` with the cached display name from `contact_profiles`.
   *
   * @category Contacts
   */
  async listContacts(): Promise<ContactRow[]> {
    const rows = await this._native.listContacts();
    return rows.map(contactRowFromNative);
  }

  /**
   * Touch a contact row, creating it if missing.
   *
   * @param curated `true` marks this as a deliberate gesture (sticky). Pass
   *                `false` to record an interaction without curating.
   *
   * @category Contacts
   */
  async touchContact(did: string, curated: boolean): Promise<void> {
    await this._native.touchContact(did, curated);
  }

  /**
   * Set or clear the local pending-message-request flag on a contact. app-core
   * no longer sets this automatically on receive; a bot that surfaces a
   * message-request inbox calls this when it receives a {@link DecryptedMessage}
   * with `isRequest === true`. Most bots don't track requests and can ignore
   * this entirely.
   *
   * @category Contacts
   */
  async setPendingRequest(did: string, pending: boolean): Promise<void> {
    await this._native.setPendingRequest(did, pending);
  }

  /**
   * Fetch the sender's encrypted profile blob using the `profileKey` from an
   * inbound {@link DecryptedMessage}, decrypt it, and cache the display name
   * locally. app-core does NOT do this automatically on receive (it would
   * block on the network and persist a name most bots never need). Best-effort
   * and idempotent. Blocks on the network.
   *
   * @category Profile
   */
  async fetchAndCacheProfile(did: string, profileKey: Uint8Array): Promise<void> {
    await this._native.fetchAndCacheProfile(did, asBuf(profileKey));
  }

  // ── groups ──────────────────────────────────────────────────────────────

  /**
   * Create a new action-bound group on this homeserver.
   *
   * @param expirySeconds Disappearing-messages timer. `0` disables it.
   * @returns The new `groupId` (URL-safe-no-pad base64) and the 32-byte
   *          master key. Stash the master key — it's the secret an invite
   *          link carries, and there is no way to recover it from the server.
   *
   * @category Groups
   */
  async createGroup(title: string, description: string, expirySeconds: number): Promise<CreatedGroup> {
    return createdGroupFromNative(await this._native.createGroup(title, description, expirySeconds));
  }

  /**
   * Pull the latest decrypted group state from the homeserver.
   *
   * @category Groups
   */
  async fetchGroupState(groupId: string): Promise<GroupSummary> {
    return groupSummaryFromNative(await this._native.fetchGroupState(groupId));
  }

  /**
   * Last-known group info from the local cache, with no server round-trip
   * (docs/53). Returns `null` if nothing is cached. Use this to show group info
   * for a group you've left, where {@link AppCore.fetchGroupState} would fail
   * the membership-gated server fetch.
   *
   * @category Groups
   */
  async cachedGroupState(groupId: string): Promise<GroupSummary | null> {
    const s = await this._native.cachedGroupState(groupId);
    return s ? groupSummaryFromNative(s) : null;
  }

  /**
   * Group ids of every group held locally — every group we have the master
   * key for, including ones we were invited to (invites are auto-accepted).
   * Reads the local group store directly, so it surfaces groups with no
   * messages yet (unlike {@link AppCore.loadConversations}). Pair with
   * {@link AppCore.fetchGroupState} to inspect membership and roles.
   *
   * @category Groups
   */
  async listGroups(): Promise<string[]> {
    return await this._native.listGroups();
  }

  /**
   * Invite `recipientDid` into the group with the given role. Also sends
   * the substrate `GroupContext` DM + Sender Key distribution message so
   * the invitee can decrypt the group on accept.
   *
   * @category Group Admin
   */
  async inviteMember(groupId: string, recipientDid: string, role: GroupRole): Promise<void> {
    await this._native.inviteMember(groupId, recipientDid, roleToNum(role));
  }

  /**
   * Accept a pending invite. Generates our own Sender Key and broadcasts
   * the distribution message to every other member.
   *
   * @category Groups
   */
  async acceptInvite(groupId: string): Promise<void> {
    await this._native.acceptInvite(groupId);
  }

  /**
   * Decline a pending invite. Removes the local pending row.
   *
   * @category Groups
   */
  async declineInvite(groupId: string): Promise<void> {
    await this._native.declineInvite(groupId);
  }

  /**
   * Join via an invite link.
   *
   * @param masterKey         32-byte master key from the link.
   * @param hostingServerUrl  Homeserver hosting the group.
   * @param password          Link password, or an empty `Uint8Array` if none.
   * @returns `"member"` (open link, admitted) or `"pending"` (RequestToJoin
   *          link, awaiting admin approval).
   *
   * @category Groups
   */
  async joinViaLink(masterKey: Uint8Array, hostingServerUrl: string, password: Uint8Array): Promise<JoinResult> {
    const r = await this._native.joinViaLink(asBuf(masterKey), hostingServerUrl, asBuf(password));
    return joinResultFromNative(r);
  }

  /**
   * Cancel a pending join request we issued via {@link AppCore.joinViaLink}.
   *
   * @category Groups
   */
  async cancelJoinRequest(groupId: string): Promise<void> {
    await this._native.cancelJoinRequest(groupId);
  }

  /**
   * Admit a requester from `pendingApprovals` into the group.
   *
   * @category Group Admin
   */
  async approveJoinRequest(groupId: string, encryptedMemberId: string): Promise<void> {
    await this._native.approveJoinRequest(groupId, encryptedMemberId);
  }

  /**
   * Reject a requester from `pendingApprovals`.
   *
   * @category Group Admin
   */
  async denyJoinRequest(groupId: string, encryptedMemberId: string): Promise<void> {
    await this._native.denyJoinRequest(groupId, encryptedMemberId);
  }

  /**
   * Remove a member from the group.
   *
   * @param encryptedMemberId From {@link GroupMember.encryptedMemberId}.
   *
   * @category Group Admin
   */
  async removeMember(groupId: string, encryptedMemberId: string): Promise<void> {
    await this._native.removeMember(groupId, encryptedMemberId);
  }

  /**
   * Leave a group yourself. Unlike {@link AppCore.removeMember} (an admin
   * action), any member can leave. Removes your server-side membership and
   * drops the group locally.
   *
   * @category Groups
   */
  async leaveGroup(groupId: string): Promise<void> {
    await this._native.leaveGroup(groupId);
  }

  /**
   * Whether the current identity is still a member of the group, per the
   * locally-cached state (docs/53). Returns `false` after leaving — the UI
   * uses this to hide the composer while keeping the conversation readable.
   *
   * @category Groups
   */
  async isGroupMember(groupId: string): Promise<boolean> {
    return await this._native.isGroupMember(groupId);
  }

  /**
   * Leave this server (docs/53): leave every group hosted here, then delete the
   * account on the server. Your identity, contacts, and other servers are
   * unaffected. Intended for non-discovery memberships.
   *
   * @category Account
   */
  async leaveServer(): Promise<void> {
    await this._native.leaveServer();
  }

  /**
   * Delete this identity from the network as completely as the protocol allows
   * (docs/53): leave-cascade every server, tombstone the DID in the PLC
   * directory, then wipe all local state. Irreversible. Throws
   * (leaving local state intact) if the PLC tombstone can't be submitted.
   *
   * @category Account
   */
  async deleteIdentity(): Promise<void> {
    await this._native.deleteIdentity();
  }

  /**
   * Change a member's role (member ↔ admin).
   *
   * @category Group Admin
   */
  async changeMemberRole(groupId: string, encryptedMemberId: string, newRole: GroupRole): Promise<void> {
    await this._native.changeMemberRole(groupId, encryptedMemberId, roleToNum(newRole));
  }

  /**
   * Set the group's disappearing-messages timer in seconds (`0` = off).
   * Admin-gated by the group's `modify_expiry` policy role (default admin),
   * so the caller must be an admin of the group.
   *
   * @category Group Admin
   */
  async setGroupExpiry(groupId: string, expirySeconds: number): Promise<void> {
    await this._native.setGroupExpiry(groupId, expirySeconds);
  }

  /**
   * Apply any pending changes from `/changes` since the last applied
   * revision.
   *
   * @returns The new local revision (equal to the previous one if nothing
   *          was pending).
   *
   * @category Groups
   */
  async applyPendingGroupChanges(groupId: string): Promise<number> {
    return await this._native.applyPendingGroupChanges(groupId);
  }

  /**
   * Encrypt and send a message to the group. Uses our Sender Key for
   * symmetric encryption, then fans out per-recipient via the existing DM
   * transport.
   *
   * @param body Text body. Sent verbatim — UTF-8 on the wire.
   *
   * @category Groups
   */
  async sendGroupMessage(groupId: string, body: string): Promise<void> {
    await this._native.sendGroupMessage(groupId, encodeBody(body));
  }

  /**
   * Generate a fresh push-routing pseudonym for this group on the server.
   * Caller should re-register with the relay using the returned bytes.
   *
   * @returns The new pseudonym bytes.
   *
   * @category Groups
   */
  async rotateGroupPseudonym(groupId: string): Promise<Uint8Array> {
    return asU8(await this._native.rotateGroupPseudonym(groupId));
  }
}

// ── PreparedAccount ─────────────────────────────────────────────────────────

/**
 * Pre-computed identity material whose DID can be known *before* server
 * registration.
 *
 * Useful when a passkey ceremony needs the DID up front (to write it into
 * the credential's user handle). Typical flow:
 *
 * ```ts
 * const prepared = await PreparedAccount.create("https://homeserver");
 * const did = prepared.did();          // e.g. "did:plc:abc..."
 * // register a passkey bound to `did` here
 * const core = await AppCore.finalizeAccount(prepared, dbPath, dbKey, recoveryKey, name);
 * ```
 *
 * A `PreparedAccount` is consumed by {@link AppCore.finalizeAccount}; calling
 * it twice with the same handle throws.
 *
 * @category Account
 */
export class PreparedAccount {
  /** @internal */ readonly _native: native.PreparedAccount;

  /** @internal */ constructor(n: native.PreparedAccount) {
    this._native = n;
  }

  /**
   * Generate identity + rotation keys and derive a `did:plc:...` locally.
   * Does not contact the homeserver.
   */
  static async create(
    serverUrl: string,
    prfOutput: Uint8Array,
  ): Promise<PreparedAccount> {
    return new PreparedAccount(
      await native.PreparedAccount.create(serverUrl, asBuf(prfOutput)),
    );
  }

  /**
   * The DID derived from the prepared keys. Empty string after this handle
   * has been consumed by {@link AppCore.finalizeAccount}.
   */
  did(): string {
    return this._native.did();
  }
}

// ── Free functions ──────────────────────────────────────────────────────────

/**
 * Install a stderr `tracing` subscriber. Idempotent — subsequent calls are
 * no-ops.
 *
 * @param filter `RUST_LOG`-style filter. Examples: `"info"`,
 *               `"app_core=debug,net=debug"`. An invalid filter falls back
 *               to `"info"`.
 *
 * @category Diagnostics
 */
export function initLogging(filter: string): void {
  native.initLogging(filter);
}

/**
 * Resolve a `did:plc:*` against the PLC directory and return the
 * homeserver URL advertised in its DID document.
 *
 * @throws For `did:local:*` (no PLC entry) or if the DID has no
 *         `AvalancheHomeserver` service entry.
 *
 * @category Diagnostics
 */
export async function resolveHomeserverFromPlc(did: string): Promise<string> {
  return await native.resolveHomeserverFromPlc(did);
}

/**
 * Download and decrypt a recovery blob from a homeserver (unauthenticated).
 *
 * @returns The decrypted server list from the blob.
 *
 * @category Account
 */
export async function downloadRecoveryBlob(
  serverUrl: string,
  did: string,
  recoveryKey: Uint8Array,
): Promise<string[]> {
  return await native.downloadRecoveryBlob(serverUrl, did, asBuf(recoveryKey));
}

/**
 * Parse and validate an invite token. Decodes locally to extract the
 * server URL, then calls the server to validate.
 *
 * @category Account
 */
export async function validateInvite(token: string): Promise<InviteInfo> {
  return inviteInfoFromNative(await native.validateInvite(token));
}
