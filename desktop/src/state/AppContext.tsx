import {
  createContext,
  createMemo,
  createSignal,
  onCleanup,
  useContext,
  type JSX,
} from "solid-js";
import { createStore, produce, reconcile } from "solid-js/store";
import { load as loadStore } from "@tauri-apps/plugin-store";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import type { Account, Conversation, InviteInfo, ServerInfo } from "../models";
import { displayHost, attachmentPlaceholder } from "../lib/format";
import { parseGroupEventMeta } from "../lib/groupEvents";
import { DeliveryStatus, type Message } from "../models/Message";
import { ServiceMode, type AvalancheService, type ConnectionState, type StoredMessageFfi, type ConversationSummaryFfi, type IncomingEvent, type ReactionFfi, type MessageRevisionFfi, type MessageTarget, type JoinResultFfi, type ContactRowFfi, type AttachmentFfi, type LinkPreviewFfi, type LinkPreviewMetaFfi } from "../services/AvalancheService";
import { MockAvalancheService } from "../services/MockAvalancheService";
import { DevServerAvalancheService } from "../services/DevServerAvalancheService";

// ── Persisted account shape (stored in tauri-plugin-store) ────────────────────

interface PersistedAccount {
  did: string;
  displayName: string;
  dbPath: string;
  servers: Array<{ id: string; name: string; url: string }>;
}

// ── Store shape ───────────────────────────────────────────────────────────────

interface AppStore {
  accounts: Account[];
  isOnboarding: boolean;
  // True while the onboarding flow is being run to ADD an account to an already
  // signed-in session ("Sign in to another account"). Distinct from
  // isOnboarding (first-run / signed-out): the main UI stays mounted underneath
  // and the existing accounts' loops keep running. Cleared by enterApp.
  isAddingAccount: boolean;
  serviceMode: ServiceMode;
  selectedTab: "chats" | "network";
  conversations: Conversation[];
  messagesByConversation: Record<string, Message[]>;
  reactionsByConversation: Record<string, ReactionFfi[]>;
  connectionStates: Record<string, ConnectionState>;
  pendingInviteToken: string | null;
  serverUrl: string;
  // Desktop-only (T72): close button hides the window to the tray instead of
  // quitting, so the WS + notifications survive. Persisted; the Rust
  // CloseRequested handler reads the same key from the plugin-store file.
  closeToTray: boolean;
}

// ── Context value ─────────────────────────────────────────────────────────────

interface AppContextValue {
  store: AppStore;
  // Account-less service for factory + pure calls (createAccount, validateInvite,
  // recoveryPhraseToSeed, …). Per-account calls go through serviceFor(accountId).
  service: () => AvalancheService;
  serviceFor: (accountId: string) => AvalancheService;
  setSelectedTab: (tab: "chats" | "network") => void;
  createAccount: (
    serverUrl: string,
    serverName: string,
    displayName: string,
    inviteToken: string | null,
    prfOutput: number[]
  ) => Promise<void>;
  restoreAccounts: () => Promise<void>;
  logout: () => void;
  serverUrl: () => string;
  setServerUrl: (url: string) => void;
  // Close-to-tray preference (T72) + manual reconnect (offline banner action).
  closeToTray: () => boolean;
  setCloseToTray: (on: boolean) => void;
  reconnectNow: () => void;
  joinServer: (
    serverUrl: string,
    serverName: string,
    existingAccountId: string
  ) => Promise<void>;
  sendMessage: (
    conversationId: string,
    text: string,
    recipientDid: string,
    senderAccountId: string
  ) => Promise<void>;
  sendGroupMessage: (conversation: Conversation, text: string) => Promise<void>;
  sendMessageWithAttachments: (
    conversation: Conversation,
    text: string,
    attachments: AttachmentFfi[],
    previews: LinkPreviewFfi[]
  ) => Promise<void>;
  uploadAttachment: (
    accountId: string,
    plaintext: number[],
    contentType: string,
    fileName: string | null,
    width: number,
    height: number,
    durationMs: number,
    thumbnail: number[],
    flags: number
  ) => Promise<AttachmentFfi>;
  downloadAttachment: (accountId: string, attachment: AttachmentFfi) => Promise<number[]>;
  fetchLinkPreview: (url: string) => Promise<LinkPreviewMetaFfi>;
  openExternal: (url: string) => Promise<void>;
  loadConversationsFromStore: () => Promise<void>;
  loadMessagesFromStore: (conversationId: string, accountId: string) => void;
  markAllMessagesRead: (conversationId: string, accountId: string) => void;
  findOrCreateDMConversation: (
    recipientDid: string,
    accountId: string
  ) => Conversation;
  aggregateConnectionState: () => ConnectionState;
  unreadCount: (conversation: Conversation) => number;
  displayName: (did: string, accountId: string) => string;
  isBot: (did: string, accountId: string) => boolean;
  setPendingInviteToken: (token: string | null) => void;
  validateInvite: (token: string) => Promise<InviteInfo>;

  // Conversation selection (lifted so compose/group flows can open a chat)
  selectedConversationId: () => string | null;
  selectConversation: (id: string | null) => void;
  reloadConversations: () => Promise<void>;
  // Reactive: latest group whose metadata changed (T74 membership re-check).
  groupMetaChange: () => { groupId: string; n: number };

  // Track A — message actions
  reactionsFor: (conversation: Conversation, message: Message) => ReactionFfi[];
  loadReactions: (conversationId: string) => void;
  toggleReaction: (conversation: Conversation, message: Message, emoji: string) => void;
  editMessage: (conversation: Conversation, message: Message, newBody: string) => void;
  loadMessageRevisions: (conversation: Conversation, message: Message) => Promise<MessageRevisionFfi[]>;
  deleteMessage: (conversation: Conversation, message: Message, forEveryone: boolean) => void;
  retryMessage: (conversation: Conversation, message: Message) => Promise<void>;

  // Track B — groups + join
  createGroupAndOpen: (
    accountId: string,
    title: string,
    recipientDids: string[],
    expirySeconds: number
  ) => Promise<Conversation>;
  joinViaLink: (
    accountId: string,
    masterKey: number[],
    hostingServerUrl: string,
    password: number[]
  ) => Promise<JoinResultFfi>;
  leaveGroup: (conversation: Conversation) => Promise<void>;

  // Track D — safety + timers
  acceptRequest: (conversation: Conversation) => Promise<void>;
  deleteRequest: (conversation: Conversation) => Promise<void>;
  reportAndBlock: (conversation: Conversation, reason: string) => Promise<void>;
  blockContact: (accountId: string, did: string) => Promise<void>;
  unblockContact: (accountId: string, did: string) => Promise<void>;
  // Aggregated across all accounts; each row carries its owning accountId.
  listBlocked: () => Promise<Array<ContactRowFfi & { accountId: string }>>;
  getConversationTimer: (accountId: string, conversationId: string) => Promise<number | null>;
  setConversationTimer: (
    accountId: string,
    recipientDid: string,
    expirySecs: number | null
  ) => Promise<void>;

  // Track E — settings / account lifecycle
  setAccountDisplayName: (accountId: string, displayName: string) => Promise<void>;
  leaveServer: (accountId: string) => Promise<void>;
  deleteIdentity: (accountId: string) => Promise<void>;
  hasRecovery: (accountId: string) => Promise<boolean>;
  generateRecoveryPhrase: () => Promise<string>;
  recoverFromPhrase: (phrase: string, serverUrl: string, displayName: string) => Promise<void>;
  // "Sign in to another account" — additive onboarding over the live session.
  startAddAccount: () => void;
  cancelAddAccount: () => void;

  // Device linking (T71). New-device side (onboarding, account-less):
  // deviceLinkShowCode (show mode) or deviceLinkEnterCode (paste mode), then
  // deviceLinkComplete polls until the account arrives (then persists + enters).
  // Existing-device side (settings, signed in): linkShowCode or linkEnterCode,
  // then linkSendBundle polls until the bundle is delivered.
  deviceLinkShowCode: () => Promise<string>;
  deviceLinkEnterCode: (code: string) => Promise<void>;
  deviceLinkComplete: () => Promise<void>;
  deviceLinkCancel: () => Promise<void>;
  linkShowCode: (accountId: string) => Promise<string>;
  linkEnterCode: (accountId: string, code: string) => Promise<void>;
  linkSendBundle: (accountId: string) => Promise<void>;

  // Deep links — route a pasted/opened link (conversation/<did>, i/<token>)
  isDeepLink: (raw: string) => boolean;
  handleDeepLink: (raw: string) => void;
}

const AppContext = createContext<AppContextValue | undefined>(undefined);

function makeService(mode: ServiceMode, accountId = ""): AvalancheService {
  return mode === ServiceMode.Mock
    ? new MockAvalancheService()
    : new DevServerAvalancheService(accountId);
}

// A conversation summary is a group iff it carries a group title or its id uses
// the `group-` prefix (DM ids are `dm-<account>-<peer>`). Single source of truth
// for the group/DM split, used by both the name-warm pass and the row builder.
function isGroupSummary(s: ConversationSummaryFfi): boolean {
  return s.groupTitle !== null || s.conversationId.startsWith("group-");
}

function messageFromFfi(m: StoredMessageFfi): Message {
  return {
    id: m.id,
    conversationId: m.conversationId,
    senderAccountId: m.senderDid,
    body: m.body,
    sentAtMs: m.sentAtMs,
    editedAtMs: m.editedAtMs ?? undefined,
    readAtMs: m.readAtMs ?? undefined,
    deliveryStatus: (m.deliveryStatus >= 0 && m.deliveryStatus <= 4
      ? m.deliveryStatus
      : DeliveryStatus.sent) as DeliveryStatus,
    editCount: m.editCount,
    isDeleted: m.deleted,
    kind: m.kind,
    metadata: m.metadata ?? undefined,
    expireTimerSecs: m.expireTimerSecs,
    expireAtMs: m.expireAtMs ?? undefined,
    attachments: m.attachments,
    previews: m.previews,
  };
}

// Build a StoredMessageFfi row for service().saveMessage from the fields that
// vary, defaulting the rest. The single source of truth for the persisted-row
// shape, shared by the optimistic-send, retry, and incoming-message paths (T75)
// so they can't drift field-by-field. `expireAtMs` is always null on write —
// app-core's reaper computes the actual expiry on read.
function buildStoredMessage(opts: {
  id: string;
  conversationId: string;
  senderDid: string;
  body: string;
  sentAtMs: number;
  deliveryStatus: DeliveryStatus;
  readAtMs?: number | null;
  editedAtMs?: number | null;
  editCount?: number;
  deleted?: boolean;
  kind?: number;
  metadata?: string | null;
  expireTimerSecs: number;
  attachments?: AttachmentFfi[];
  previews?: LinkPreviewFfi[];
}): StoredMessageFfi {
  return {
    id: opts.id,
    conversationId: opts.conversationId,
    senderDid: opts.senderDid,
    body: opts.body,
    sentAtMs: opts.sentAtMs,
    editedAtMs: opts.editedAtMs ?? null,
    readAtMs: opts.readAtMs ?? null,
    deliveryStatus: opts.deliveryStatus,
    editCount: opts.editCount ?? 0,
    deleted: opts.deleted ?? false,
    kind: opts.kind ?? 0,
    metadata: opts.metadata ?? null,
    expireTimerSecs: opts.expireTimerSecs,
    expireAtMs: null,
    attachments: opts.attachments ?? [],
    previews: opts.previews ?? [],
  };
}

// Delivery-status progression rank: sending(0) → sent(1) → delivered(2) → read(3).
// `failed` gets -1 so a failure only applies from a non-terminal state and is
// never treated as "more advanced" than read. Used by applyDeliveryStatusUpdates
// to ensure receipts only ever move a message forward.
function deliveryRank(s: DeliveryStatus): number {
  switch (s) {
    case DeliveryStatus.sending:   return 0;
    case DeliveryStatus.sent:      return 1;
    case DeliveryStatus.delivered: return 2;
    case DeliveryStatus.read:      return 3;
    case DeliveryStatus.failed:    return -1;
  }
}

// ── Provider ──────────────────────────────────────────────────────────────────

export function AppProvider(props: { children: JSX.Element }) {
  const [store, setStore] = createStore<AppStore>({
    accounts: [],
    isOnboarding: true,
    isAddingAccount: false,
    serviceMode: ServiceMode.DevServer,
    selectedTab: "chats",
    conversations: [],
    messagesByConversation: {},
    reactionsByConversation: {},
    connectionStates: {},
    pendingInviteToken: null,
    serverUrl: "http://localhost:3000",
    // Default on: closing keeps the app alive in the tray so messages keep
    // arriving (matches the Rust-side default in close_to_tray_enabled).
    closeToTray: true,
  });

  // One AvalancheService per signed-in account, keyed by accountId — the desktop
  // analog of iOS/Android's `cores` map (all identities share one inbox; there
  // is no "active" account). `serviceFor(accountId)` resolves the per-account
  // service: a DevServer instance binds its accountId into every Tauri command;
  // a Mock instance holds that account's own seeded state. Account-less factory
  // and pure calls (createAccount, validateInvite, recoveryPhraseToSeed, …) use
  // `onboardingService()`.
  const services = new Map<string, AvalancheService>();
  // The service for the account currently being added (createAccount / login /
  // recover / device-link). For Mock it accumulates that account's seeded state
  // and becomes the account's service on success (then we rotate a fresh one for
  // the next add); for DevServer it's an unbound instance used only for the
  // account-less calls above. Per-instance, never a module global
  // (desktop/CLAUDE.md "Mock/dev services hold per-instance state").
  let onboardingSvc: AvalancheService = makeService(store.serviceMode);

  function onboardingService(): AvalancheService {
    return onboardingSvc;
  }

  function serviceFor(accountId: string): AvalancheService {
    const existing = services.get(accountId);
    if (existing) return existing;
    // DevServer is stateless per account — bind lazily so a restored account
    // resolves even before registerAccountService runs. For Mock, a missing
    // entry means it was never registered (the seeded state lives in the
    // instance), so fall back to the onboarding instance keyed under this id.
    if (store.serviceMode === ServiceMode.DevServer) {
      const bound = new DevServerAvalancheService(accountId);
      services.set(accountId, bound);
      return bound;
    }
    services.set(accountId, onboardingSvc);
    return onboardingSvc;
  }

  // Register the just-created/restored account's service. Mock: the onboarding
  // instance carries the seeded state, so it becomes this account's service and
  // we rotate a fresh one for the next add. DevServer: bind a fresh instance.
  function registerAccountService(accountId: string) {
    if (store.serviceMode === ServiceMode.Mock) {
      services.set(accountId, onboardingSvc);
      onboardingSvc = makeService(ServiceMode.Mock);
    } else {
      services.set(accountId, new DevServerAvalancheService(accountId));
    }
  }

  // Selected conversation — lifted into context so compose/group/join flows
  // can programmatically open a conversation. ChatsView mirrors this signal.
  const [selectedConversationId, setSelectedConversationId] = createSignal<string | null>(null);

  // Bumps each time a group's metadata changes (incoming GroupMetadataChanged),
  // carrying the affected groupId. ConversationView tracks this to re-check
  // membership for the open group without waiting for a conversation switch (T74).
  const [groupMetaChange, setGroupMetaChange] = createSignal<{ groupId: string; n: number }>({
    groupId: "",
    n: 0,
  });

  // Reactive display-name cache: reads are tracked by Solid so components
  // re-render when a resolved name arrives.  A separate plain Set tracks
  // in-flight fetches to prevent duplicate IPC calls per DID.
  const [displayNameCache, setDisplayNameCache] = createStore<Record<string, string>>({});
  const displayNamePending: Set<string> = new Set();

  // Reactive is-bot cache, same pattern as displayNameCache: components read it
  // in a tracking scope so a bot avatar (hexagon) resolves once getAccountInfo
  // returns. A plain Set guards against duplicate in-flight fetches per DID.
  const [isBotCache, setIsBotCache] = createStore<Record<string, boolean>>({});
  const isBotPending: Set<string> = new Set();

  // Load-once guards
  const loadedConversations = { value: false };
  const loadedMessages: Set<string> = new Set();
  // Coalesces forced conversation reloads (the inbound-event handlers plus
  // safety/group actions) so their store reconciles don't interleave. A reload
  // requested while one is in flight queues exactly one follow-up rather than
  // launching a second interleaving load.
  let reloadInFlight: Promise<void> | null = null;
  let reloadQueued = false;
  const loadedReactions: Set<string> = new Set();
  // Conversation ids created in-memory (e.g. an incoming DM in a brand-new
  // thread) that aren't yet backed by a row in the local DB.
  // loadConversationsFromStore preserves only these across a reload, NOT
  // arbitrary DB-absent entries, which would resurrect conversations the DB
  // intentionally dropped. The incoming-message handler persists the received
  // message (so the conversation appears in the DB summaries on the next
  // reload), and this set bridges the brief gap until that reload runs; the
  // drop-on-DB-appearance path below then hands it back to normal lifecycle.
  const pendingConversations: Set<string> = new Set();

  // Event + connection loop lifecycle, one of each per account (mirrors iOS
  // eventTasks/stateTasks and Android eventJobs/stateJobs). A loop runs while its
  // accountId is in `loops`; teardown removes the id and clears any pending retry
  // timer. `startPollingFor` is idempotent (guards on map membership), so the
  // restore + add-account paths can both call it without spawning duplicate loops
  // that would process every event twice (desktop/CLAUDE.md "Background loops").
  type LoopHandle = {
    eventRunning: boolean;
    connRunning: boolean;
    eventTimeout?: ReturnType<typeof setTimeout>;
  };
  const loops = new Map<string, LoopHandle>();

  // ── Helpers ────────────────────────────────────────────────────────────────

  // Resolve the owning accountId of a conversation already in the store. Used by
  // the few context methods that receive only a conversationId (not the whole
  // Conversation) to route per-account service calls. Returns null if unknown.
  function accountIdForConversation(conversationId: string): string | null {
    return store.conversations.find((c) => c.id === conversationId)?.accountId ?? null;
  }

  function getServerUrl(accountId: string): string {
    // Uses `servers[0]`, the server the account was created with / joined first,
    // which stands in for the account's discovery/home server until multi-server
    // is modeled. There is no `isCurrent` field on ServerInfo (the original code
    // that referenced one was a bug that silently returned "").
    // TODO(multi-server): per docs/53, an identity has one discovery/home server
    // plus additional memberships; there is no single user-selected "current
    // server". When multi-server lands, account-level calls should use the
    // discovery server and group/DM operations should use the conversation's own
    // server, rather than a global current-server flag.
    return (
      store.accounts
        .find((a) => a.id === accountId)
        ?.servers[0]?.url ?? ""
    );
  }

  function recipientDidFromConvId(
    convId: string,
    accountId: string
  ): string | null {
    const prefix = `dm-${accountId}-`;
    if (convId.startsWith(prefix)) return convId.slice(prefix.length);
    return null;
  }

  // ── Persistence helpers ───────────────────────────────────────────────────

  async function persistedAccounts(): Promise<PersistedAccount[]> {
    try {
      const s = await loadStore("avalanche.json");
      return (await s.get<PersistedAccount[]>("accounts")) ?? [];
    } catch {
      return [];
    }
  }

  async function persistAccounts(accounts: PersistedAccount[]) {
    try {
      const s = await loadStore("avalanche.json");
      await s.set("accounts", accounts);
      await s.save();
    } catch {}
  }

  async function addPersistedAccount(pa: PersistedAccount) {
    const existing = await persistedAccounts();
    const filtered = existing.filter((a) => a.did !== pa.did);
    await persistAccounts([...filtered, pa]);
  }

  async function persistServerUrl(url: string) {
    try {
      const s = await loadStore("avalanche.json");
      await s.set("serverUrl", url);
      await s.save();
    } catch {}
  }

  function setServerUrl(url: string) {
    setStore("serverUrl", url);
    void persistServerUrl(url);
  }

  // Persist the close-to-tray toggle to the same plugin-store file the Rust
  // CloseRequested handler reads (`close_to_tray_enabled`).
  async function persistCloseToTray(on: boolean) {
    try {
      const s = await loadStore("avalanche.json");
      await s.set("closeToTray", on);
      await s.save();
    } catch {}
  }

  function setCloseToTray(on: boolean) {
    setStore("closeToTray", on);
    void persistCloseToTray(on);
  }

  // Manual "Reconnect now" (offline banner). Best-effort — wakes every signed-in
  // account's reconnect loop (mirrors iOS, which drives connectivity per core).
  function reconnectNow() {
    for (const account of store.accounts) {
      void serviceFor(account.id)
        .reconnectNow()
        .catch(() => { /* signed out / unavailable */ });
    }
  }

  // ── Init: read persisted mode on mount ───────────────────────────────────

  void (async () => {
    try {
      const s = await loadStore("avalanche.json");
      const savedServerUrl = await s.get<string>("serverUrl");
      // Service mode is no longer user-selectable — mock is a test-only affordance
      // (constructed directly in tests), not a runtime mode users pick. The live
      // app always runs DevServer (the store default). We intentionally ignore any
      // previously persisted "mock" so a dev who toggled it before isn't stranded
      // with no UI to switch back.
      if (savedServerUrl != null) {
        setStore("serverUrl", savedServerUrl);
      }
      const savedCloseToTray = await s.get<boolean>("closeToTray");
      if (savedCloseToTray != null) {
        setStore("closeToTray", savedCloseToTray);
      }
    } catch {}
  })();

  // ── Deep-link listener ────────────────────────────────────────────────────
  // Single consumer of `avalanche-deeplink` (emitted by the Rust deep-link
  // plugin, see src-tauri/src/lib.rs). OnboardingFlow's pendingInviteToken
  // effect still drives onboarding navigation for invite tokens.
  let deeplinkUnlisten: (() => void) | undefined;
  listen<string>("avalanche-deeplink", (ev) => handleDeepLink(ev.payload))
    .then((un) => { deeplinkUnlisten = un; })
    .catch(() => { /* Tauri event API unavailable (browser/test) */ });
  onCleanup(() => deeplinkUnlisten?.());

  // ── Foreground hook ───────────────────────────────────────────────────────
  // On window focus, tell the core the app is active (T77). This wakes the
  // reconnect loop and probes a possibly-dead socket — so after the machine
  // resumes from sleep or a network change, a stale connection (and any
  // messages sent/read on another linked device meanwhile) recovers promptly
  // via the reconnect + storage resync, without a restart.
  //
  // We deliberately do NOT deactivate on blur, unlike iOS's scenePhase gating.
  // Desktop has no OS suspension and no push fallback, and close-to-tray
  // explicitly promises that messages/notifications keep arriving while the
  // window is hidden — so the keepalive (and its dead-socket detection) must
  // stay on whenever the process is running. The core defaults to active, so
  // never calling `setAppActive(false)` keeps the connection alive for the
  // app's lifetime; focus is purely an opportunistic reconnect trigger.
  // No-op before sign-in: the command short-circuits when there's no account.
  let focusUnlisten: (() => void) | undefined;
  getCurrentWindow()
    .onFocusChanged(({ payload: focused }) => {
      if (focused) {
        // Push foreground-active to every account (iOS setIsAppActive loops all
        // cores) so each one's keepalive + opportunistic reconnect fires.
        for (const account of store.accounts) {
          void serviceFor(account.id)
            .setAppActive(true)
            .catch(() => { /* offline / signed out */ });
        }
      }
    })
    .then((un) => { focusUnlisten = un; })
    .catch(() => { /* Tauri window API unavailable (browser/test) */ });
  onCleanup(() => focusUnlisten?.());

  // ── Account lifecycle ─────────────────────────────────────────────────────

  // Shared completion step for every onboarding path: resets the conversation
  // load guard, loads conversations, starts event/connection loops, and clears
  // the onboarding flag.  All three paths (createAccount, restoreAccounts,
  // joinServer) must call this — never inline the steps individually.
  function enterApp() {
    loadedConversations.value = false;
    void loadConversationsFromStore();
    // Idempotent per account: existing loops are not restarted; a newly added
    // account's loops start here. This is what makes "sign in another account"
    // additive — the first account's loops keep running untouched.
    for (const account of store.accounts) startPollingFor(account.id);
    setStore("isOnboarding", false);
    setStore("isAddingAccount", false);
  }

  // Only restore once per session.  SplashView.onMount fires on every
  // back-stack push, so guard against a second concurrent or repeat call.
  let restoring = false;
  let restored = false;

  async function restoreAccounts() {
    // Never re-restore once any account is signed in — SplashView.onMount fires
    // restoreAccounts, and that splash is also shown by the "Sign in to another
    // account" overlay over a live session. Without the accounts-present guard,
    // re-mounting it would re-login (re-open the DB of) accounts that are already
    // running. The create/recover paths handle adding accounts additively.
    if (restoring || restored || store.accounts.length > 0) return;
    restoring = true;

    try {
      const persisted = await persistedAccounts();
      if (persisted.length === 0) return;

      for (const p of persisted) {
        try {
          // login is account-less (returns the DID); register the per-account
          // service under that DID so every later call routes to its core.
          const result = await onboardingService().login(p.dbPath, "dev-placeholder-key");
          registerAccountService(result.did);
          const account: Account = {
            id: result.did,
            displayName: result.displayName || p.displayName,
            avatarData: null,
            servers: p.servers.map((srv) => ({
              id: srv.id,
              name: srv.name,
              url: srv.url,
              displayHost: displayHost(srv.url, srv.name),
            })),
          };
          // Skip duplicates — store may already contain this account if
          // restoreAccounts is called again mid-session.
          if (!store.accounts.some((a) => a.id === result.did)) {
            setStore("accounts", (prev) => [...prev, account]);
          }
        } catch {
          // Account login failed — skip; leave persisted for next launch.
        }
      }

      if (store.accounts.length > 0) {
        restored = true;
        enterApp();
      }
    } finally {
      restoring = false;
    }
  }

  async function createAccount(
    serverUrl: string,
    serverName: string,
    displayName: string,
    inviteToken: string | null,
    prfOutput: number[]
  ) {
    const dbPath = `account-${Math.random().toString(36).slice(2, 10)}.db`;
    const result = await onboardingService().createAccount(
      serverUrl,
      dbPath,
      // DB key: a placeholder until OS-keychain integration. Mirrors mobile's
      // "dev-placeholder-key" (iOS uses the Secure Enclave; desktop has no
      // equivalent wired yet).
      "dev-placeholder-key",
      // Desktop has no WebAuthn passkey, so signup derives the recovery seed
      // from a BIP39 phrase the user writes down (RecoveryPhraseSetupView) and
      // passes it here as the PRF output — exactly iOS's phrase-account mode.
      // This makes the rotation key + DID reproducible from the phrase, so
      // recover_from_phrase can locate and decrypt the recovery blob later.
      prfOutput,
      displayName,
      inviteToken
    );
    // Bind this account's service before any per-account call routes to it.
    registerAccountService(result.did);

    const serverInfo: ServerInfo = {
      id: serverUrl,
      name: serverName,
      url: serverUrl,
      displayHost: displayHost(serverUrl, serverName),
    };

    const account: Account = {
      id: result.did,
      displayName: result.displayName || displayName,
      avatarData: null,
      servers: [serverInfo],
    };

    setStore("accounts", (prev) => [...prev, account]);

    await addPersistedAccount({
      did: result.did,
      displayName: account.displayName,
      dbPath,
      servers: [{ id: serverUrl, name: serverName, url: serverUrl }],
    });

    enterApp();
  }

  // ── Device linking (T71) ────────────────────────────────────────────────────
  // Poll cadence mirrors iOS AppState (1s interval, 180s deadline). The TS layer
  // drives the loop so it stays cancellable, per docs/04 §4.2 (no long-lived,
  // uncancellable FFI call).
  const LINK_POLL_MS = 1000;
  const LINK_TIMEOUT_MS = 180_000;

  // New device, show mode: generate this device's pairing code to display.
  // Account-less (no account yet) → onboarding service.
  async function deviceLinkShowCode(): Promise<string> {
    return onboardingService().deviceLinkCreatePairing(null);
  }

  // New device, paste mode: accept the existing device's pairing code.
  async function deviceLinkEnterCode(code: string): Promise<void> {
    await onboardingService().deviceLinkAcceptPairing(code);
  }

  // New device: poll until the provisioning bundle arrives, then install the
  // linked account and enter the app — the same completion as createAccount
  // (account row + persisted record + enterApp). The home server is learned
  // from the bundle (homeServer()), not from user input.
  async function deviceLinkComplete(): Promise<void> {
    const dbPath = `account-${Math.random().toString(36).slice(2, 10)}.db`;
    const deadline = Date.now() + LINK_TIMEOUT_MS;
    for (;;) {
      const result = await onboardingService().deviceLinkAwaitStep(dbPath, "dev-placeholder-key");
      if (result) {
        // The backend has installed the linked core keyed by this DID; bind its
        // service so homeServer() (per-account) and the loops route correctly.
        registerAccountService(result.did);
        const serverUrl = await serviceFor(result.did).homeServer();
        const serverInfo: ServerInfo = {
          id: serverUrl,
          name: serverUrl,
          url: serverUrl,
          displayHost: displayHost(serverUrl, serverUrl),
        };
        const account: Account = {
          id: result.did,
          displayName: result.displayName,
          avatarData: null,
          servers: [serverInfo],
        };
        if (!store.accounts.some((a) => a.id === result.did)) {
          setStore("accounts", (prev) => [...prev, account]);
        }
        await addPersistedAccount({
          did: result.did,
          displayName: account.displayName,
          dbPath,
          servers: [{ id: serverUrl, name: serverUrl, url: serverUrl }],
        });
        enterApp();
        return;
      }
      if (Date.now() >= deadline) {
        await onboardingService().deviceLinkReset().catch(() => {});
        throw new Error("Device link timed out. Please try again.");
      }
      await new Promise((r) => setTimeout(r, LINK_POLL_MS));
    }
  }

  // New device: abandon an in-progress pairing (view teardown / cancel).
  async function deviceLinkCancel(): Promise<void> {
    await onboardingService().deviceLinkReset().catch(() => {});
  }

  // Existing device, show mode: generate this device's pairing code to display.
  // Per-account: the user is linking a new device to a specific identity.
  async function linkShowCode(accountId: string): Promise<string> {
    return serviceFor(accountId).linkCreatePairing(null);
  }

  // Existing device, paste mode: accept the new device's pairing code.
  async function linkEnterCode(accountId: string, code: string): Promise<void> {
    await serviceFor(accountId).linkAcceptPairing(code);
  }

  // Existing device: poll until the provisioning bundle has been sealed + sent.
  async function linkSendBundle(accountId: string): Promise<void> {
    const deadline = Date.now() + LINK_TIMEOUT_MS;
    for (;;) {
      const done = await serviceFor(accountId).linkSendBundleStep();
      if (done) return;
      if (Date.now() >= deadline) {
        throw new Error("Device link timed out. Please try again.");
      }
      await new Promise((r) => setTimeout(r, LINK_POLL_MS));
    }
  }

  function resetSession() {
    // Full logout: stop every account's loops and drop every core (each old
    // reconnect task + WS connection dies on drop). The TS loops are torn down
    // first, so clearSession just releases the backend cores.
    stopPolling();
    for (const account of store.accounts) {
      serviceFor(account.id).clearSession().catch(() => {});
    }
    services.clear();
    // Fresh onboarding service so mock state (storedMessages, pendingEvents,
    // mockDid) can't bleed into the next session — never a module global.
    onboardingSvc = makeService(store.serviceMode);
    // Block restoreAccounts from re-entering while we clear persisted state.
    // Otherwise SplashView.onMount fires restoreAccounts before persistAccounts([])
    // completes, finding stale accounts and auto-signing-in — undoing the logout.
    restoring = true;
    setStore(
      produce((s) => {
        s.accounts = [];
        s.isOnboarding = true;
        s.conversations = [];
        s.messagesByConversation = {};
        s.reactionsByConversation = {};
        s.connectionStates = {};
        s.pendingInviteToken = null;
      })
    );
    setSelectedConversationId(null);
    loadedConversations.value = false;
    loadedMessages.clear();
    loadedReactions.clear();
    pendingConversations.clear();
    // Reset the reactive display-name cache so components get a reactive
    // update on logout/mode-switch.
    setDisplayNameCache(reconcile({}));
    displayNamePending.clear();
    setIsBotCache(reconcile({}));
    isBotPending.clear();
    // Clear persisted accounts, then release the restore guard so a
    // subsequent manual restore or fresh session can proceed cleanly.
    void persistAccounts([]).finally(() => {
      restoring = false;
      restored = false;
    });
  }

  function logout() {
    // resetSession already drops every core, clears the services map, and rotates
    // a fresh onboarding service — nothing more to do for a full sign-out.
    resetSession();
  }

  async function joinServer(
    serverUrl: string,
    serverName: string,
    existingAccountId: string
  ) {
    const idx = store.accounts.findIndex((a) => a.id === existingAccountId);
    if (idx >= 0) {
      setStore("accounts", idx, "servers", (prev) => [
        ...prev,
        { id: serverUrl, name: serverName, url: serverUrl, displayHost: displayHost(serverUrl, serverName) },
      ]);
      // Persist the new server so it survives restart.  We re-read persisted
      // accounts rather than relying on the in-memory snapshot, because the
      // account may have accumulated additional state (e.g. display name from
      // login) since it was last written.
      const persisted = await persistedAccounts();
      const existingIdx = persisted.findIndex((pa) => pa.did === existingAccountId);
      if (existingIdx >= 0) {
        persisted[existingIdx].servers.push({
          id: serverUrl,
          name: serverName,
          url: serverUrl,
        });
        await persistAccounts(persisted);
      }
    }
    enterApp();
  }

  // ── Track E: settings / account lifecycle ──────────────────────────────────

  // Remove ONE account from the device after a server-side leave or identity
  // delete, leaving the other signed-in accounts running (shared-inbox model).
  // If it was the last account, fall back to a full reset → onboarding.
  async function removeAccountLocally(accountId: string) {
    stopPollingFor(accountId);
    // Drop the backend core (no-op if delete_identity already removed it).
    await serviceFor(accountId).clearSession().catch(() => {});
    services.delete(accountId);
    setStore(
      produce((s) => {
        s.accounts = s.accounts.filter((a) => a.id !== accountId);
        delete s.connectionStates[accountId];
      })
    );
    const persisted = await persistedAccounts();
    await persistAccounts(persisted.filter((p) => p.did !== accountId));
    if (store.accounts.length === 0) {
      resetSession();
    } else {
      // Rebuild the merged conversation list from the remaining accounts.
      await reloadConversations();
    }
  }

  // Update the user's display name on the core, then mirror it into the in-memory
  // account and the persisted entry so it survives a restart.
  async function setAccountDisplayName(accountId: string, displayName: string) {
    const trimmed = displayName.trim();
    if (!trimmed) return;
    await serviceFor(accountId).setDisplayName(trimmed);
    const idx = store.accounts.findIndex((a) => a.id === accountId);
    if (idx >= 0) setStore("accounts", idx, "displayName", trimmed);
    const persisted = await persistedAccounts();
    const pIdx = persisted.findIndex((p) => p.did === accountId);
    if (pIdx >= 0) {
      persisted[pIdx].displayName = trimmed;
      await persistAccounts(persisted);
    }
  }

  // Mirrors iOS AppState.leaveServer: leave the connected server on the core,
  // then drop the account from the device. The UI only offers this for non-home
  // memberships (ServerDetailView gates it). Throws on failure, leaving the
  // account in place so the user can retry.
  async function leaveServer(accountId: string) {
    await serviceFor(accountId).leaveServer();
    await removeAccountLocally(accountId);
  }

  // Mirrors iOS AppState.deleteIdentity: the core leaves every server, submits a
  // PLC tombstone, and wipes local rows; then we drop the account from the
  // device. Throws (leaving state intact) if the tombstone couldn't be submitted.
  async function deleteIdentity(accountId: string) {
    await serviceFor(accountId).deleteIdentity();
    await removeAccountLocally(accountId);
  }

  function hasRecovery(accountId: string): Promise<boolean> {
    return serviceFor(accountId).hasRecovery().catch((e: unknown) => {
      console.warn("hasRecovery failed:", e);
      return false;
    });
  }

  function generateRecoveryPhrase(): Promise<string> {
    return onboardingService().generateRecoveryPhrase();
  }

  // ── Deep links (T61) ────────────────────────────────────────────────────────

  // Parse a deep-link URL into (action, arg), accepting both the custom
  // `avalanche://<action>/<arg>` scheme (what the desktop OS launches) and the
  // universal-link form `https://go.theavalanche.net/<action>/<arg>` (iOS parity).
  function parseDeepLink(raw: string): { action: string; arg: string } | null {
    let url: URL;
    try {
      url = new URL(raw);
    } catch {
      return null;
    }
    let segments: string[];
    if (url.protocol === "avalanche:") {
      // avalanche://a/b puts the first segment in `host`; the triple-slash form
      // avalanche:///a/b puts everything in the path. Handle both.
      segments = [url.host, ...url.pathname.split("/")].filter(Boolean);
    } else if (url.host === "go.theavalanche.net") {
      segments = url.pathname.split("/").filter(Boolean);
    } else {
      return null;
    }
    if (segments.length < 2) return null;
    return { action: segments[0], arg: segments.slice(1).join("/") };
  }

  // Decode a base64url invite token ({s:serverUrl,d:inviterDid}) — the decode
  // side of lib/format.makeInviteToken, matching iOS handleDeepLink.
  function decodeInviteToken(
    token: string
  ): { serverUrl: string; inviterDid: string | null } | null {
    try {
      const b64 = token.replace(/-/g, "+").replace(/_/g, "/");
      // Restore the padding makeInviteToken strips, so atob decodes reliably
      // regardless of webview base64 strictness.
      const padded = b64 + "=".repeat((4 - (b64.length % 4)) % 4);
      const obj = JSON.parse(atob(padded)) as { s?: unknown; d?: unknown };
      if (typeof obj.s !== "string") return null;
      return { serverUrl: obj.s, inviterDid: typeof obj.d === "string" ? obj.d : null };
    } catch {
      return null;
    }
  }

  const trimSlashes = (s: string) => s.replace(/\/+$/, "");

  // True if `raw` is a routable deep link (an avalanche:// or
  // go.theavalanche.net URL with a recognized action) — lets the recipient
  // field distinguish a pasted contact/invite link from a bare DID.
  function isDeepLink(raw: string): boolean {
    return parseDeepLink(raw) !== null;
  }

  // Route a deep link like iOS AppState.handleDeepLink: open a DM for
  // conversation/<did>, and for i/<token> jump straight to the inviter's DM if
  // already on that server, else hand the token to onboarding.
  function handleDeepLink(raw: string) {
    const parsed = parseDeepLink(raw);
    if (!parsed) return;
    const { action, arg } = parsed;

    if (action === "conversation") {
      // No conversation context in a bare contact link, so default to the first
      // account (iOS does the same for ambiguous deep links — AppState.swift).
      const accountId = store.accounts[0]?.id ?? null;
      if (!arg || accountId === null) return;
      const conv = findOrCreateDMConversation(arg, accountId);
      setStore("selectedTab", "chats");
      setSelectedConversationId(conv.id);
      return;
    }

    if (action === "i" || action === "invite") {
      const token = arg;
      const decoded = decodeInviteToken(token);
      if (decoded) {
        const account = store.accounts.find((a) =>
          a.servers.some((s) => trimSlashes(s.url) === trimSlashes(decoded.serverUrl))
        );
        if (account && decoded.inviterDid) {
          // Already on this server — open the inviter's DM directly.
          const conv = findOrCreateDMConversation(decoded.inviterDid, account.id);
          setStore("selectedTab", "chats");
          setSelectedConversationId(conv.id);
          return;
        }
      }
      // Not on the server (or undecodable) → start onboarding with the token.
      setStore("pendingInviteToken", token);
    }
  }

  // Recovery RESTORE: recompute the DID from the phrase seed + home server URL,
  // then restore the account from its recovery blob. Mirrors iOS
  // RecoveryExplainerView.recoverWithPhrase → recoverAccount. On success the
  // account is added and the app enters the main UI (same path as createAccount).
  async function recoverFromPhrase(phrase: string, serverUrl: string, displayName: string) {
    // recoveryPhraseToSeed / deriveDidFromPasskey / recoverFromPhrase are all
    // account-less (no core yet) → onboarding service.
    const seed = await onboardingService().recoveryPhraseToSeed(phrase);
    const did = await onboardingService().deriveDidFromPasskey(seed, serverUrl);
    if (store.accounts.some((a) => a.id === did)) {
      throw new Error("This identity is already signed in on this device.");
    }
    const dbPath = `account-${Math.random().toString(36).slice(2, 10)}.db`;
    const result = await onboardingService().recoverFromPhrase(
      phrase,
      serverUrl,
      did,
      dbPath,
      "dev-placeholder-key",
      displayName
    );
    // Bind the restored account's service before per-account calls route to it.
    registerAccountService(result.did);
    const serverInfo: ServerInfo = {
      id: serverUrl,
      name: serverUrl,
      url: serverUrl,
      displayHost: displayHost(serverUrl, serverUrl),
    };
    const account: Account = {
      id: result.did,
      displayName: result.displayName || displayName || `Account ${result.did.slice(-6)}`,
      avatarData: null,
      servers: [serverInfo],
    };
    setStore("accounts", (prev) => [...prev, account]);
    await addPersistedAccount({
      did: result.did,
      displayName: account.displayName,
      dbPath,
      servers: [{ id: serverUrl, name: serverUrl, url: serverUrl }],
    });
    enterApp();
  }

  // ── Messaging ─────────────────────────────────────────────────────────────

  // DIDs whose display names should be warmed from local storage before the
  // chat list renders (T78, mirrors iOS displayNameDidsToWarm): DM peers, group
  // last-message senders, and the actor/target of a group system-event preview.
  function displayNameDidsToWarm(
    summaries: ConversationSummaryFfi[],
    accountId: string
  ): string[] {
    const dids = new Set<string>();
    for (const s of summaries) {
      if (isGroupSummary(s)) {
        const last = s.lastMessage;
        if (!last) continue;
        if (last.senderDid) dids.add(last.senderDid);
        if (last.kind > 0) {
          // System-event previews resolve actor/target DIDs (e.g. "Alice made
          // Bob an admin"), so warm those too.
          const m = parseGroupEventMeta(last.metadata ?? undefined);
          if (m?.actor_did) dids.add(m.actor_did);
          if (m?.target_did) dids.add(m.target_did);
        }
      } else {
        const recipientDid = recipientDidFromConvId(s.conversationId, accountId);
        if (recipientDid) dids.add(recipientDid);
      }
    }
    return Array.from(dids);
  }

  async function loadConversationsFromStore() {
    if (loadedConversations.value) return;
    loadedConversations.value = true;

    if (store.accounts.length === 0) {
      // No account signed in — reset the guard so a later load (once an account
      // enters) isn't permanently suppressed.
      loadedConversations.value = false;
      setStore("conversations", []);
      return;
    }

    // Build the merged inbox across every signed-in account (shared-inbox model).
    // Group conversation ids (`group-<groupId>`) are NOT account-scoped — matching
    // iOS/Android — so a group two of your identities both belong to dedups to a
    // single row owned by the first account that materializes it (`groupSeen`).
    // DM ids embed the owning account (`dm-<accountId>-<peer>`) so they're already
    // globally unique. Each account's rows are loaded from that account's own core.
    // Fetch every account's summaries (and warm its name cache) concurrently —
    // the loads are independent per core, so N accounts cost ~1× latency, not N×.
    // Processing below stays in account order so the group dedup is deterministic.
    const accountsList = store.accounts.slice();
    const perAccount = await Promise.all(
      accountsList.map(async (account) => {
        const accountId = account.id;
        const summaries = await serviceFor(accountId)
          .loadConversations()
          .catch(() => [] as ConversationSummaryFfi[]);
        // Warm the display-name cache from this account's local store (no network)
        // before building titles — avoids DM rows flashing the raw DID on cold
        // launch (T78, mirrors iOS displayNameDidsToWarm + cachedDisplayNames).
        const warmDids = displayNameDidsToWarm(summaries, accountId);
        if (warmDids.length) {
          try {
            const names = await serviceFor(accountId).cachedDisplayNames(warmDids);
            for (const [did, name] of Object.entries(names)) {
              if (name) setDisplayNameCache(did, name);
            }
          } catch {
            // Local-only warm; a failure just means rows resolve via the async path.
          }
        }
        return { account, summaries };
      })
    );

    const all: Conversation[] = [];
    const groupSeen = new Set<string>();
    for (const { account, summaries } of perAccount) {
      const accountId = account.id;
      const serverUrl = getServerUrl(accountId);
      for (const s of summaries) {
        const isGroup = isGroupSummary(s);
        // Dedup shared groups across accounts: first account to surface it wins.
        if (isGroup && groupSeen.has(s.conversationId)) continue;
        if (isGroup) groupSeen.add(s.conversationId);

        const groupId = s.conversationId.startsWith("group-")
          ? s.conversationId.slice("group-".length)
          : undefined;
        const recipientDid = !isGroup
          ? recipientDidFromConvId(s.conversationId, accountId) ?? undefined
          : undefined;
        const title = isGroup
          ? s.groupTitle ?? "Group"
          : displayNameCache[recipientDid ?? ""] ?? recipientDid ?? s.conversationId;

        // Caption-less attachment messages have an empty body — preview them as
        // "Photo"/"Attachment" using the summary's attachment content type (iOS
        // chat-list parity).
        const lastBody = s.lastMessage?.body ?? "";
        const lastPreview =
          lastBody.trim().length === 0 && s.lastMessageAttachmentContentType
            ? attachmentPlaceholder(s.lastMessageAttachmentContentType)
            : s.lastMessage?.body ?? undefined;

        all.push({
          id: s.conversationId,
          title,
          accountId,
          serverUrl,
          recipientDid,
          groupId,
          lastMessage: lastPreview,
          lastMessageDate: s.lastMessage?.sentAtMs ?? undefined,
          lastMessageKind: s.lastMessage?.kind ?? 0,
          lastMessageMetadata: s.lastMessage?.metadata ?? undefined,
          lastMessageSenderDid: s.lastMessage?.senderDid ?? undefined,
          isGroup,
          isRequest: s.isRequest,
          isBlocked: s.isBlocked,
          // Authoritative unread seed from core (excludes own + expired). (A5)
          unreadCount: s.unreadCount,
        });
      }
    }

    const dbIds = new Set(all.map((c) => c.id));
    // A pending conversation that now appears in the DB is fully persisted —
    // stop tracking it so it follows normal DB-driven lifecycle from here on.
    for (const id of dbIds) pendingConversations.delete(id);
    // Preserve only still-unpersisted in-memory conversations. Other DB-absent
    // entries (e.g. a group the DB dropped after leaving) are intentionally let go.
    const preserved = store.conversations.filter(
      (c) => !dbIds.has(c.id) && pendingConversations.has(c.id)
    );
    const merged = [...all, ...preserved].sort(
      (a, b) => (b.lastMessageDate ?? 0) - (a.lastMessageDate ?? 0)
    );
    setStore("conversations", merged);
  }

  // Force a fresh conversation reload, bypassing the load-once guard. Used by
  // the inbound-event handlers (and later by safety/group actions). If a reload
  // is already running, queue exactly one follow-up rather than launching a
  // second interleaving load — this is the "please reload" path, distinct from
  // the load-once `loadConversationsFromStore`.
  function reloadConversations(): Promise<void> {
    if (reloadInFlight) {
      reloadQueued = true;
      return reloadInFlight;
    }
    loadedConversations.value = false;
    reloadInFlight = loadConversationsFromStore().finally(() => {
      reloadInFlight = null;
      if (reloadQueued) {
        reloadQueued = false;
        void reloadConversations();
      }
    });
    return reloadInFlight;
  }

  // Reload a conversation's timeline from the store, fully replacing the
  // in-memory copy (matches iOS `reloadMessagesIfLoaded`). Only acts on an
  // already-loaded conversation, so it never eagerly loads unopened ones. A full
  // replace is correct because the store is the source of truth: a row missing
  // from the reload was deliberately deleted (expired by the disappearing-
  // messages reaper, docs/03 §5, or tombstoned), so it must leave the UI too.
  function reloadMessagesIfLoaded(cid: string) {
    if (!loadedMessages.has(cid)) return;
    const accountId = accountIdForConversation(cid);
    if (accountId === null) return;
    void serviceFor(accountId)
      .loadMessages(cid)
      .then((rows) => {
        setStore("messagesByConversation", cid, rows.map(messageFromFfi));
      })
      .catch((e: unknown) => {
        console.warn("reloadMessagesIfLoaded failed:", cid, e);
      });
  }

  function loadMessagesFromStore(conversationId: string, accountId: string) {
    if (loadedMessages.has(conversationId)) return;
    loadedMessages.add(conversationId);

    void serviceFor(accountId)
      .loadMessages(conversationId)
      .then((rows) => {
        const messages = rows.map(messageFromFfi);
        if (messages.length > 0) {
          setStore("messagesByConversation", conversationId, messages);
        }
      })
      .catch((err) => {
        console.warn("loadMessages failed for", conversationId, err);
        loadedMessages.delete(conversationId);
      });
  }

  async function sendOptimistic(
    conversationId: string,
    text: string,
    senderAccountId: string,
    expireTimerSecs: number,
    transportFn: (sentAtMs: number) => Promise<void>,
    errorMessage: string,
    attachments?: AttachmentFfi[],
    previews?: LinkPreviewFfi[]
  ) {
    const messageId = crypto.randomUUID();
    const sentAtMs = Date.now();

    const optimistic: Message = {
      id: messageId,
      conversationId,
      senderAccountId,
      body: text,
      sentAtMs,
      readAtMs: sentAtMs,
      deliveryStatus: DeliveryStatus.sending,
      editCount: 0,
      isDeleted: false,
      kind: 0,
      // Stamp the sender's local copy with the same disappearing-messages timer
      // the wire envelope carries, so app-core's reaper expires it too. Without
      // this the sender's own messages never disappear (docs/03 §5).
      expireTimerSecs,
      attachments,
      previews,
    };

    setStore("messagesByConversation", conversationId, (prev) => [
      ...(prev ?? []),
      optimistic,
    ]);

    // Update conversation preview. Clear any stale group system-event fields so
    // ConversationRow renders this new message, not a prior "X joined" line.
    // Attachment-only sends (empty body) preview as "Photo"/"Attachment" (iOS parity).
    const previewText =
      text.trim().length > 0
        ? text
        : attachmentPlaceholder(attachments?.[0]?.contentType);
    const convIdx = store.conversations.findIndex((c) => c.id === conversationId);
    if (convIdx >= 0) {
      setStore("conversations", convIdx, "lastMessage", previewText);
      setStore("conversations", convIdx, "lastMessageDate", sentAtMs);
      setStore("conversations", convIdx, "lastMessageKind", 0);
      setStore("conversations", convIdx, "lastMessageMetadata", undefined);
    }

    // Persist every delivery state to the local store so the timeline survives a
    // refresh/restart and a store reload never loses an in-flight or failed
    // send. "sending" is persisted up front (and awaited) so a crash or refresh
    // mid-send is recoverable. Matches iOS AppState.sendMessage.
    const persist = (status: DeliveryStatus) =>
      serviceFor(senderAccountId).saveMessage(
        buildStoredMessage({
          id: messageId,
          conversationId,
          senderDid: senderAccountId,
          body: text,
          sentAtMs,
          readAtMs: sentAtMs,
          deliveryStatus: status,
          expireTimerSecs,
          attachments,
          previews,
        })
      );

    await persist(DeliveryStatus.sending);

    try {
      await transportFn(sentAtMs);
      setStore("messagesByConversation", conversationId, (msgs) =>
        (msgs ?? []).map((m) =>
          m.id === messageId
            ? { ...m, deliveryStatus: DeliveryStatus.sent }
            : m
        )
      );
      // Best-effort re-save — log failures but never crash the send path.
      void persist(DeliveryStatus.sent).catch((err: unknown) => {
        console.warn("saveMessage (sent) failed:", err);
      });
    } catch {
      setStore("messagesByConversation", conversationId, (msgs) =>
        (msgs ?? []).map((m) =>
          m.id === messageId
            ? { ...m, deliveryStatus: DeliveryStatus.failed }
            : m
        )
      );
      void persist(DeliveryStatus.failed).catch((err: unknown) => {
        console.warn("saveMessage (failed) failed:", err);
      });
      throw new Error(errorMessage);
    }
  }

  async function sendMessage(
    conversationId: string,
    text: string,
    recipientDid: string,
    senderAccountId: string
  ) {
    // DM disappearing-messages timer is keyed by peer DID in app-core's store
    // (save/load_conversation_expiry), so read it with recipientDid — not the
    // full conversation id — to match what the wire envelope is stamped with.
    const timer =
      (await serviceFor(senderAccountId).getConversationTimer(recipientDid).catch(() => null)) ?? 0;
    await sendOptimistic(
      conversationId,
      text,
      senderAccountId,
      timer,
      (sentAtMs) =>
        serviceFor(senderAccountId).sendDm(
          recipientDid,
          Array.from(new TextEncoder().encode(text)),
          sentAtMs
        ),
      "Send failed"
    );
  }

  async function sendGroupMessage(conversation: Conversation, text: string) {
    if (!conversation.groupId) return;
    const groupId = conversation.groupId;
    const svc = serviceFor(conversation.accountId);
    const timer = (await svc.groupExpirySeconds(groupId).catch(() => 0)) ?? 0;
    await sendOptimistic(
      conversation.id,
      text,
      conversation.accountId,
      timer,
      (sentAtMs) => svc.sendGroupMessage(groupId, Array.from(new TextEncoder().encode(text)), sentAtMs),
      "Group send failed"
    );
  }

  // Send a message carrying attachments and/or link previews to either a DM or a
  // group — one path for both targets (mirrors iOS sendWithAttachments). Routes
  // through sendOptimistic so the local row, persistence, and delivery states
  // behave identically to a plain send. The body may be empty (attachment-only).
  async function sendMessageWithAttachments(
    conversation: Conversation,
    text: string,
    attachments: AttachmentFfi[],
    previews: LinkPreviewFfi[]
  ) {
    const target = messageTargetFor(conversation);
    const svc = serviceFor(conversation.accountId);
    const timer =
      target.type === "group"
        ? (await svc.groupExpirySeconds(target.group_id).catch(() => 0)) ?? 0
        : (await svc.getConversationTimer(target.recipient_did).catch(() => null)) ?? 0;
    await sendOptimistic(
      conversation.id,
      text,
      conversation.accountId,
      timer,
      (sentAtMs) =>
        svc.sendMessageWithAttachments(target, text, attachments, previews, sentAtMs),
      "Send failed",
      attachments,
      previews
    );
  }

  // ── Attachments / link previews / external links (thin service pass-throughs,
  // kept on the context so views never reach the service directly and Mock mode
  // keeps working) ─────────────────────────────────────────────────────────────
  // Attachments encrypt/decrypt with the owning account's keys, so they route to
  // that account's core (accountId threaded from the conversation in the view).
  function uploadAttachment(
    accountId: string,
    plaintext: number[],
    contentType: string,
    fileName: string | null,
    width: number,
    height: number,
    durationMs: number,
    thumbnail: number[],
    flags: number
  ): Promise<AttachmentFfi> {
    return serviceFor(accountId).uploadAttachment(
      plaintext,
      contentType,
      fileName,
      width,
      height,
      durationMs,
      thumbnail,
      flags
    );
  }

  function downloadAttachment(accountId: string, attachment: AttachmentFfi): Promise<number[]> {
    return serviceFor(accountId).downloadAttachment(attachment);
  }

  // Link-preview fetch + external-open are pure Rust commands (no core) → account-less.
  function fetchLinkPreview(url: string): Promise<LinkPreviewMetaFfi> {
    return onboardingService().fetchLinkPreview(url);
  }

  function openExternal(url: string): Promise<void> {
    return onboardingService().openExternal(url);
  }

  function markAllMessagesRead(conversationId: string, accountId: string) {
    // Optimistically clear the seeded unread badge on open (A5). Also flips the
    // reactive dep so the chat-list badge re-renders as the transcript loads.
    const ci = store.conversations.findIndex((c) => c.id === conversationId);
    if (ci >= 0 && store.conversations[ci].unreadCount) {
      setStore("conversations", ci, "unreadCount", 0);
    }
    const msgs = store.messagesByConversation[conversationId];
    if (!msgs) return;
    const now = Date.now();
    let changed = false;
    // Collect the sent-at timestamps of inbound messages that newly flip to
    // read, so we can acknowledge them to the sender via a read receipt.
    const newlyReadSentAt: number[] = [];
    const updated = msgs.map((m) => {
      if (m.readAtMs === undefined && m.senderAccountId !== accountId) {
        changed = true;
        newlyReadSentAt.push(m.sentAtMs);
        return { ...m, readAtMs: now };
      }
      return m;
    });
    if (changed) {
      setStore("messagesByConversation", conversationId, updated);
      void serviceFor(accountId)
        .markMessagesRead(conversationId, now)
        .catch((e: unknown) => {
          console.warn("markMessagesRead failed:", e);
        });
      // Send a read receipt to the DM partner so their bubbles flip to "read".
      // Receipts are 1:1 (a single recipient), so this applies to DMs only —
      // a group has no single recipient. Suppress for un-accepted requests
      // (opening to evaluate isn't acknowledgement) and for blocked contacts
      // (never signal "read" to someone you cut off) — don't rely on app-core
      // to gate it.
      const conv = store.conversations.find((c) => c.id === conversationId);
      if (
        conv &&
        !conv.isGroup &&
        !conv.isRequest &&
        !conv.isBlocked &&
        conv.recipientDid &&
        newlyReadSentAt.length > 0
      ) {
        const recipientDid = conv.recipientDid;
        void serviceFor(accountId)
          .sendReadReceipt(recipientDid, newlyReadSentAt)
          .catch((e: unknown) => {
            console.warn("sendReadReceipt failed:", e);
          });
      }
    }
  }

  function findOrCreateDMConversation(
    recipientDid: string,
    accountId: string
  ): Conversation {
    const existing = store.conversations.find(
      (c) => c.accountId === accountId && c.recipientDid === recipientDid
    );
    if (existing) return existing;

    const serverUrl = getServerUrl(accountId);
    const convId = `dm-${accountId}-${recipientDid}`;
    // Trigger async fetch; title updates reactively when the cache populates.
    const title = displayName(recipientDid, accountId);
    const conv: Conversation = {
      id: convId,
      title,
      accountId,
      serverUrl,
      recipientDid,
      isGroup: false,
      isRequest: false,
      isBlocked: false,
      lastMessageKind: 0,
    };
    // Track as pending so a reload (e.g. a storageSynced event firing before
    // the first message persists) doesn't drop this freshly-opened, selected
    // conversation — loadConversationsFromStore only preserves DB-absent
    // conversations that are in pendingConversations.
    pendingConversations.add(convId);
    setStore("conversations", (prev) => [...prev, conv]);
    return conv;
  }

  function findOrCreateGroupConversation(
    groupId: string,
    title: string,
    accountId: string
  ): Conversation {
    const convId = `group-${groupId}`;
    const existing = store.conversations.find((c) => c.id === convId);
    if (existing) return existing;
    const conv: Conversation = {
      id: convId,
      title,
      accountId,
      serverUrl: getServerUrl(accountId),
      groupId,
      isGroup: true,
      isRequest: false,
      isBlocked: false,
      lastMessageKind: 0,
    };
    // See findOrCreateDMConversation — a just-created group isn't in the DB
    // summaries yet, so preserve it across reloads until its state syncs.
    pendingConversations.add(convId);
    setStore("conversations", (prev) => [...prev, conv]);
    return conv;
  }

  function messageTargetFor(conversation: Conversation): MessageTarget {
    return conversation.isGroup && conversation.groupId
      ? { type: "group", group_id: conversation.groupId }
      : { type: "dm", recipient_did: conversation.recipientDid ?? "" };
  }

  // ── Track A: reactions / edit / delete / retry ─────────────────────────────

  function reactionsFor(
    conversation: Conversation,
    message: Message
  ): ReactionFfi[] {
    const all = store.reactionsByConversation[conversation.id] ?? [];
    return all.filter(
      (r) =>
        r.targetAuthor === message.senderAccountId &&
        r.targetSentAtMs === message.sentAtMs
    );
  }

  function loadReactions(conversationId: string) {
    if (loadedReactions.has(conversationId)) return;
    const accountId = accountIdForConversation(conversationId);
    if (accountId === null) return;
    loadedReactions.add(conversationId);
    void serviceFor(accountId)
      .loadReactions(conversationId)
      .then((rows) => {
        setStore("reactionsByConversation", conversationId, rows);
      })
      .catch((err: unknown) => {
        console.warn("loadReactions failed for", conversationId, err);
        loadedReactions.delete(conversationId);
      });
  }

  function toggleReaction(
    conversation: Conversation,
    message: Message,
    emoji: string
  ) {
    const myDid = conversation.accountId;
    const convId = conversation.id;
    const targetAuthor = message.senderAccountId;
    const targetSentAtMs = message.sentAtMs;
    const now = Date.now();

    const current = store.reactionsByConversation[convId] ?? [];
    const existingMine = current.find(
      (r) =>
        r.targetAuthor === targetAuthor &&
        r.targetSentAtMs === targetSentAtMs &&
        r.reactorDid === myDid
    );
    const remove = existingMine?.emoji === emoji;

    // Optimistic in-memory update: drop my prior reaction on this message,
    // then (unless toggling the same emoji off) add the new one.
    const withoutMine = current.filter(
      (r) =>
        !(
          r.targetAuthor === targetAuthor &&
          r.targetSentAtMs === targetSentAtMs &&
          r.reactorDid === myDid
        )
    );
    const next = remove
      ? withoutMine
      : [
          ...withoutMine,
          {
            conversationId: convId,
            targetAuthor,
            targetSentAtMs,
            reactorDid: myDid,
            emoji,
            reactedAtMs: now,
          },
        ];
    setStore("reactionsByConversation", convId, next);

    void serviceFor(conversation.accountId)
      .sendReaction(
        messageTargetFor(conversation),
        targetAuthor,
        targetSentAtMs,
        emoji,
        remove,
        now
      )
      .catch((e: unknown) => {
        console.warn("sendReaction failed:", e);
      });
  }

  function editMessage(
    conversation: Conversation,
    message: Message,
    newBody: string
  ) {
    const trimmed = newBody.trim();
    if (!trimmed || trimmed === message.body) return;
    const now = Date.now();
    setStore("messagesByConversation", conversation.id, (prev) =>
      (prev ?? []).map((m) =>
        m.id === message.id
          ? {
              ...m,
              body: trimmed,
              editedAtMs: now,
              editCount: m.editCount + 1,
            }
          : m
      )
    );
    // Keep the sidebar preview in sync when the edited message is the latest
    // (the inbound messageEdited handler does the same for peers' edits).
    const convIdx = store.conversations.findIndex((c) => c.id === conversation.id);
    if (
      convIdx >= 0 &&
      store.conversations[convIdx]?.lastMessageDate === message.sentAtMs
    ) {
      const preview = trimmed.length > 100 ? trimmed.slice(0, 100) + "…" : trimmed;
      setStore("conversations", convIdx, "lastMessage", preview);
    }
    void serviceFor(conversation.accountId)
      .sendEdit(messageTargetFor(conversation), message.sentAtMs, trimmed, now)
      .catch((e: unknown) => {
        console.warn("sendEdit failed:", e);
      });
  }

  function loadMessageRevisions(
    conversation: Conversation,
    message: Message
  ): Promise<MessageRevisionFfi[]> {
    return serviceFor(conversation.accountId)
      .loadMessageRevisions(
        conversation.id,
        message.senderAccountId,
        message.sentAtMs
      )
      .catch((e: unknown) => {
        console.warn("loadMessageRevisions failed:", e);
        return [] as MessageRevisionFfi[];
      });
  }

  function clearReactionsForMessage(
    conversationId: string,
    targetAuthor: string,
    targetSentAtMs: number
  ) {
    const current = store.reactionsByConversation[conversationId];
    if (!current) return;
    setStore(
      "reactionsByConversation",
      conversationId,
      current.filter(
        (r) =>
          !(
            r.targetAuthor === targetAuthor &&
            r.targetSentAtMs === targetSentAtMs
          )
      )
    );
  }

  function deleteMessage(
    conversation: Conversation,
    message: Message,
    forEveryone: boolean
  ) {
    const now = Date.now();
    if (forEveryone) {
      setStore("messagesByConversation", conversation.id, (prev) =>
        (prev ?? []).map((m) =>
          m.id === message.id
            ? { ...m, body: "", isDeleted: true, editedAtMs: undefined }
            : m
        )
      );
    } else {
      setStore("messagesByConversation", conversation.id, (prev) =>
        (prev ?? []).filter((m) => m.id !== message.id)
      );
    }
    clearReactionsForMessage(
      conversation.id,
      message.senderAccountId,
      message.sentAtMs
    );
    void serviceFor(conversation.accountId)
      .sendDelete(
        messageTargetFor(conversation),
        message.senderAccountId,
        message.sentAtMs,
        forEveryone,
        now
      )
      .catch((e: unknown) => {
        console.warn("sendDelete failed:", e);
      });
  }

  async function retryMessage(conversation: Conversation, message: Message) {
    // Flip back to "sending", re-run the transport with a fresh timestamp, and
    // resolve to sent/failed exactly like the original optimistic send.
    const sentAtMs = Date.now();
    setStore("messagesByConversation", conversation.id, (prev) =>
      (prev ?? []).map((m) =>
        m.id === message.id
          ? { ...m, deliveryStatus: DeliveryStatus.sending, sentAtMs }
          : m
      )
    );
    // Persist each state (same contract as the original optimistic send) so the
    // retried message survives a reload regardless of outcome. Matches iOS.
    const svc = serviceFor(conversation.accountId);
    const persist = (status: DeliveryStatus) =>
      svc.saveMessage(
        buildStoredMessage({
          id: message.id,
          conversationId: conversation.id,
          senderDid: message.senderAccountId,
          body: message.body,
          sentAtMs,
          readAtMs: sentAtMs,
          deliveryStatus: status,
          editedAtMs: message.editedAtMs ?? null,
          editCount: message.editCount,
          deleted: message.isDeleted,
          kind: message.kind,
          metadata: message.metadata ?? null,
          expireTimerSecs: message.expireTimerSecs,
        })
      );
    await persist(DeliveryStatus.sending);
    const bytes = Array.from(new TextEncoder().encode(message.body));
    try {
      if (conversation.isGroup && conversation.groupId) {
        await svc.sendGroupMessage(conversation.groupId, bytes, sentAtMs);
      } else if (conversation.recipientDid) {
        await svc.sendDm(conversation.recipientDid, bytes, sentAtMs);
      } else {
        throw new Error("no transport target");
      }
      setStore("messagesByConversation", conversation.id, (prev) =>
        (prev ?? []).map((m) =>
          m.id === message.id
            ? { ...m, deliveryStatus: DeliveryStatus.sent }
            : m
        )
      );
      void persist(DeliveryStatus.sent).catch((err: unknown) => {
        console.warn("saveMessage after retry failed:", err);
      });
    } catch (e) {
      setStore("messagesByConversation", conversation.id, (prev) =>
        (prev ?? []).map((m) =>
          m.id === message.id
            ? { ...m, deliveryStatus: DeliveryStatus.failed }
            : m
        )
      );
      void persist(DeliveryStatus.failed).catch((err: unknown) => {
        console.warn("saveMessage (failed) after retry failed:", err);
      });
      console.warn("retryMessage failed:", e);
    }
  }

  // ── Track B: groups + join via link ────────────────────────────────────────

  async function createGroupAndOpen(
    accountId: string,
    title: string,
    recipientDids: string[],
    expirySeconds: number
  ): Promise<Conversation> {
    const svc = serviceFor(accountId);
    const created = await svc.createGroup(title, "", expirySeconds);
    const groupId = created.groupId;
    // Best-effort fan-out: one failed invite must not abort the rest.
    for (const did of recipientDids) {
      try {
        await svc.inviteMember(groupId, did, 0);
      } catch (e) {
        console.warn("inviteMember failed for", did, e);
      }
    }
    const conv = findOrCreateGroupConversation(groupId, title, accountId);
    return conv;
  }

  // The pasted-/opened-link entry (NewConversationView → handleDeepLink) covers
  // contact and server-invite links (conversation/<did>, i/<token>). A *group*
  // join link — which would carry the master key + invite-link password
  // (docs/03 §3.10) into this handler — has no defined URL wire format in iOS or
  // app-core yet, so there is deliberately no UI that builds those args here.
  // Wire it once that format exists; the handler itself is complete.
  async function joinViaLink(
    accountId: string,
    masterKey: number[],
    hostingServerUrl: string,
    password: number[]
  ): Promise<JoinResultFfi> {
    const result = await serviceFor(accountId).joinViaLink(masterKey, hostingServerUrl, password);
    await reloadConversations();
    return result;
  }

  async function leaveGroup(conversation: Conversation) {
    if (!conversation.groupId) return;
    try {
      await serviceFor(conversation.accountId).leaveGroup(conversation.groupId);
    } catch (e) {
      // Don't flip the UI to the irreversible read-only "you left" state when
      // the server-side leave didn't actually happen — the user is still a
      // member and can keep participating.
      console.warn("leaveGroup failed:", e);
      throw e;
    }
    // Keep the conversation visible but read-only (Signal-style): mark it left
    // and track it as pending so the next loadConversationsFromStore preserves
    // it if app-core stops returning the left group.
    const idx = store.conversations.findIndex((c) => c.id === conversation.id);
    if (idx >= 0) setStore("conversations", idx, "hasLeft", true);
    pendingConversations.add(conversation.id);
  }

  // ── Track D: message requests / blocking / timers ──────────────────────────

  async function acceptRequest(conversation: Conversation) {
    if (!conversation.recipientDid) return;
    await serviceFor(conversation.accountId)
      .acceptRequest(conversation.recipientDid)
      .catch((e: unknown) => {
        console.warn("acceptRequest failed:", e);
      });
    await reloadConversations();
  }

  async function deleteRequest(conversation: Conversation) {
    if (!conversation.recipientDid) return;
    await serviceFor(conversation.accountId)
      .deleteRequest(conversation.recipientDid)
      .catch((e: unknown) => {
        console.warn("deleteRequest failed:", e);
      });
    if (selectedConversationId() === conversation.id) setSelectedConversationId(null);
    await reloadConversations();
  }

  async function reportAndBlock(conversation: Conversation, reason: string) {
    if (!conversation.recipientDid) return;
    await serviceFor(conversation.accountId)
      .reportAndBlock(conversation.recipientDid, reason)
      .catch((e: unknown) => {
        console.warn("reportAndBlock failed:", e);
      });
    await reloadConversations();
  }

  async function blockContact(accountId: string, did: string) {
    await serviceFor(accountId).blockContact(did).catch((e: unknown) => {
      console.warn("blockContact failed:", e);
    });
    await reloadConversations();
  }

  async function unblockContact(accountId: string, did: string) {
    await serviceFor(accountId).unblockContact(did).catch((e: unknown) => {
      console.warn("unblockContact failed:", e);
    });
    await reloadConversations();
  }

  // Blocked contacts are per-account; aggregate across every signed-in account so
  // the settings list shows them all, each row tagged with its owning accountId.
  async function listBlocked(): Promise<Array<ContactRowFfi & { accountId: string }>> {
    // Per-account lists are independent — fetch them concurrently, then flatten.
    const perAccount = await Promise.all(
      store.accounts.map(async (account) => {
        const accountRows = await serviceFor(account.id)
          .listBlocked()
          .catch((e: unknown) => {
            console.warn("listBlocked failed:", e);
            return [] as ContactRowFfi[];
          });
        return accountRows.map((r) => ({ ...r, accountId: account.id }));
      })
    );
    return perAccount.flat();
  }

  function getConversationTimer(
    accountId: string,
    conversationId: string
  ): Promise<number | null> {
    return serviceFor(accountId).getConversationTimer(conversationId).catch((e: unknown) => {
      console.warn("getConversationTimer failed:", e);
      return null;
    });
  }

  async function setConversationTimer(
    accountId: string,
    recipientDid: string,
    expirySecs: number | null
  ) {
    await serviceFor(accountId).setConversationTimer(recipientDid, expirySecs).catch((e: unknown) => {
      console.warn("setConversationTimer failed:", e);
    });
  }

  function unreadCount(conversation: Conversation): number {
    // Read messagesByConversation unconditionally so this accessor always tracks
    // it reactively — otherwise a conversation opened with 0 unread never
    // establishes the dependency and its badge goes stale when a later message
    // arrives. Once the transcript is loaded, per-message read state is
    // authoritative (and reflects optimistic clears); before that, use the count
    // seeded from ConversationSummaryFfi.unreadCount (core excludes own +
    // expired). Mirrors iOS unreadCount(for:).
    const msgs = store.messagesByConversation[conversation.id];
    if (loadedMessages.has(conversation.id)) {
      return (msgs ?? []).filter(
        (m) => m.readAtMs === undefined && m.senderAccountId !== conversation.accountId
      ).length;
    }
    return conversation.unreadCount ?? 0;
  }

  function displayName(did: string, accountId: string): string {
    const own = store.accounts.find((a) => a.id === did);
    if (own) return own.displayName;
    // Reactive read: Solid tracks this access so components re-render when
    // the cache is populated by the async fetch below.
    const cached = displayNameCache[did];
    if (cached !== undefined) return cached;
    // Guard against duplicate in-flight fetches for the same DID.
    if (!displayNamePending.has(did)) {
      displayNamePending.add(did);
      void serviceFor(accountId)
        .contactDisplayName(did)
        .then((name) => {
          // Always cache — even empty strings — to prevent infinite refetch.
          // Only update conversation titles when a non-empty name arrives.
          setDisplayNameCache(did, name);
          if (name) {
            store.conversations.forEach((c, i) => {
              if (c.recipientDid === did && c.title === did) {
                setStore("conversations", i, "title", name);
              }
            });
          }
        })
        .catch((e: unknown) => {
          console.warn("contactDisplayName failed:", did, e);
        })
        .finally(() => {
          displayNamePending.delete(did);
        });
    }
    return did;
  }

  // Reactive is-bot lookup (docs/54 bot presentation). Returns the cached value
  // (default false) and fires a getAccountInfo fetch to populate it; the read is
  // tracked by Solid, so a bot avatar re-renders as a hexagon once resolved. Own
  // accounts are never bots. Mirrors the displayName cache pattern.
  function isBot(did: string, accountId: string): boolean {
    if (store.accounts.some((a) => a.id === did)) return false;
    const cached = isBotCache[did];
    if (cached !== undefined) return cached;
    if (!isBotPending.has(did)) {
      isBotPending.add(did);
      void serviceFor(accountId)
        .getAccountInfo(did)
        .then((info) => setIsBotCache(did, info.isBot))
        .catch((e: unknown) => {
          console.warn("getAccountInfo (isBot) failed:", did, e);
        })
        .finally(() => {
          isBotPending.delete(did);
        });
    }
    return false;
  }

  // Fire a native notification for an inbound message (mirrors iOS
  // NotificationPresenter.present). Suppressed when the user is already viewing
  // this conversation in a focused window; shown otherwise (window unfocused, or
  // focused on a different conversation). Permission is requested on first use.
  async function maybeNotify(conversationId: string, senderDid: string, body: string) {
    const text = body.trim();
    if (!text) return;
    try {
      const focused = await getCurrentWindow().isFocused().catch(() => false);
      if (focused && selectedConversationId() === conversationId) return;
      let granted = await isPermissionGranted();
      if (!granted) granted = (await requestPermission()) === "granted";
      if (!granted) return;
      const title = displayNameCache[senderDid] || senderDid;
      const preview = text.length > 120 ? text.slice(0, 120) + "…" : text;
      sendNotification({ title, body: preview });
    } catch (e) {
      console.warn("notification failed:", e);
    }
  }

  // ── Event loop ────────────────────────────────────────────────────────────

  // Drain a batch of decrypted events (mirrors iOS `AppState.eventLoop`,
  // AppState.swift). The switch only *collects*; state is applied once after the
  // loop, so a whole batch triggers at most one conversation reload — this is
  // what kills the interleaving-reload races the old per-case reloads caused.
  // Point mutations that don't benefit from batching (edits, deletes) are
  // applied inline via small named handlers, matching iOS.
  // `accountId` is the account whose event loop delivered this batch — every
  // event here belongs to that account (used to attribute incoming messages /
  // new conversations to the right identity in the shared inbox).
  function handleIncomingEvents(events: IncomingEvent[], accountId: string) {
    const messages: Extract<IncomingEvent, { type: "message" }>["msg"][] = [];
    const receiptUpdates: Extract<
      IncomingEvent,
      { type: "receiptUpdate" }
    >["update"][] = [];
    let needsConversationReload = false;

    for (const ev of events) {
      switch (ev.type) {
        case "message":
          messages.push(ev.msg);
          break;
        case "receiptUpdate":
          receiptUpdates.push(ev.update);
          break;
        case "messageEdited": {
          const e = ev as Extract<IncomingEvent, { type: "messageEdited" }>;
          applyInboundEdit(
            e.conversation_id ?? "",
            e.author_did,
            e.sent_at_ms,
            e.new_body,
            e.edited_at_ms
          );
          break;
        }
        case "messageDeleted": {
          const d = ev as Extract<IncomingEvent, { type: "messageDeleted" }>;
          applyInboundDelete(d.conversation_id ?? "", d.author_did, d.sent_at_ms);
          break;
        }
        case "reactionUpdated": {
          const r = ev as Extract<IncomingEvent, { type: "reactionUpdated" }>;
          applyInboundReaction(
            r.conversation_id,
            r.target_author,
            r.target_sent_at_ms,
            r.reactor_did,
            r.emoji,
            r.removed
          );
          break;
        }
        case "messagesExpired": {
          const exp = ev as Extract<IncomingEvent, { type: "messagesExpired" }>;
          for (const cid of exp.conversation_ids) reloadMessagesIfLoaded(cid);
          needsConversationReload = true;
          break;
        }
        case "groupMetadataChanged": {
          // app-core has already persisted a system row (e.g. "X made Y an
          // admin") for this change. Refresh the group's timeline (if loaded)
          // so it appears, then refresh the conversation list/preview.
          const gm = ev as Extract<IncomingEvent, { type: "groupMetadataChanged" }>;
          reloadMessagesIfLoaded(`group-${gm.event.groupId}`);
          // Notify any open ConversationView for this group to re-check
          // membership (e.g. you were removed by another admin while viewing).
          setGroupMetaChange((p) => ({ groupId: gm.event.groupId, n: p.n + 1 }));
          needsConversationReload = true;
          break;
        }
        case "conversationUpdated": {
          // A `SyncSent`/`SyncRead` transcript from another of my own linked
          // devices changed exactly this conversation's stored content (a
          // message I sent, an edit/delete/reaction I made, or read-state I
          // cleared). Re-read just this timeline so it surfaces live, then
          // refresh the chat-list preview. Mirrors iOS `conversationUpdated`.
          const cu = ev as Extract<IncomingEvent, { type: "conversationUpdated" }>;
          reloadMessagesIfLoaded(cu.conversation_id);
          needsConversationReload = true;
          break;
        }
        case "groupInvite":
        case "storageSynced":
          needsConversationReload = true;
          break;
        default:
          console.warn(
            "handleIncomingEvents: unknown event type",
            (ev as { type: string }).type
          );
          break;
      }
    }

    // Apply phase — run once for the whole batch, attributed to this loop's
    // account.
    for (const m of messages) handleIncomingMessage(m, accountId);
    if (receiptUpdates.length) applyDeliveryStatusUpdates(receiptUpdates);
    if (needsConversationReload) void reloadConversations();
  }

  // Append or reconcile a single decrypted message (iOS `handleIncomingMessage`).
  function handleIncomingMessage(
    m: Extract<IncomingEvent, { type: "message" }>["msg"],
    accountId: string
  ) {
    const conversationId = m.groupId
      ? `group-${m.groupId}`
      : `dm-${accountId}-${m.senderDid}`;
    const senderIsSelf = m.senderDid === accountId;

    if (senderIsSelf && m.sentAtMs !== null) {
      // Echo of our own outgoing message — update the optimistic entry in-place
      // by sentAtMs instead of appending a duplicate. Only match messages still
      // in a non-terminal delivery state (sending/sent); a delivered message is
      // already confirmed and should not be matched again.
      setStore("messagesByConversation", conversationId, (prev) =>
        (prev ?? []).map((existing) =>
          existing.sentAtMs === m.sentAtMs &&
          existing.senderAccountId === accountId &&
          (existing.deliveryStatus === DeliveryStatus.sending ||
            existing.deliveryStatus === DeliveryStatus.sent)
            ? {
                ...existing,
                deliveryStatus: DeliveryStatus.delivered,
                id: `server-${m.serverId}`,
              }
            : existing
        )
      );
      return;
    }

    // Received from another user — append as a new message.
    const body = new TextDecoder().decode(new Uint8Array(m.plaintext));
    const msg: Message = {
      id: crypto.randomUUID(),
      conversationId,
      senderAccountId: m.senderDid,
      body,
      sentAtMs: m.sentAtMs ?? Date.now(),
      deliveryStatus: DeliveryStatus.delivered,
      editCount: 0,
      isDeleted: false,
      kind: 0,
      expireTimerSecs: m.expireTimerSecs,
      attachments: m.attachments,
      previews: m.previews,
    };
    setStore("messagesByConversation", conversationId, (prev) => [
      ...(prev ?? []),
      msg,
    ]);
    // Chat-list preview: real text, else "Photo"/"Attachment" for an
    // attachment-only message (iOS parity).
    const previewSource =
      body.trim().length > 0
        ? body
        : attachmentPlaceholder(m.attachments[0]?.contentType);
    const previewText =
      previewSource.length > 100 ? previewSource.slice(0, 100) + "…" : previewSource;
    // Seed/bump the unread badge for conversations whose transcript isn't loaded
    // (the loaded case is counted from per-message read state). (A5)
    if (!loadedMessages.has(conversationId)) {
      const ci = store.conversations.findIndex((c) => c.id === conversationId);
      if (ci >= 0) {
        setStore("conversations", ci, "unreadCount", (n) => (n ?? 0) + 1);
      }
    }
    // Fire a native notification for the inbound message (suppressed when the
    // user is already viewing this conversation in a focused window).
    void maybeNotify(conversationId, m.senderDid, body);
    // Persist the incoming message. app-core does NOT persist messages on the
    // receive path (the client owns local history), so without this every
    // received message is lost on app restart/refresh while sent messages
    // (which are saved) survive. readAtMs stays null (unread) until the
    // conversation is opened; the store starts the disappearing-messages
    // countdown on read. Mirrors iOS AppState.
    void serviceFor(accountId)
      .saveMessage(
        buildStoredMessage({
          id: msg.id,
          conversationId,
          senderDid: m.senderDid,
          body,
          sentAtMs: msg.sentAtMs,
          readAtMs: null,
          deliveryStatus: msg.deliveryStatus,
          expireTimerSecs: m.expireTimerSecs,
          attachments: m.attachments,
          previews: m.previews,
        })
      )
      .catch((err: unknown) => {
        console.warn("saveMessage (incoming) failed:", err);
      });

    // Update conversation preview, or create the conversation in-memory.
    const convIdx = store.conversations.findIndex((c) => c.id === conversationId);
    if (convIdx >= 0) {
      setStore("conversations", convIdx, "lastMessage", previewText);
      setStore(
        "conversations",
        convIdx,
        "lastMessageDate",
        m.sentAtMs ?? Date.now()
      );
      // Clear stale group system-event fields (see sendOptimistic).
      setStore("conversations", convIdx, "lastMessageKind", 0);
      setStore("conversations", convIdx, "lastMessageMetadata", undefined);
    } else {
      // Conversation not in the list yet — create it in-memory. The incoming
      // message is now persisted (above), so it will also show up in the DB
      // summaries on the next reload; tracking it pending bridges the gap.
      const isGroup = !!m.groupId;
      // A DM from a sender that didn't pass the message-request gate is a
      // request (docs/12 §1). app-core reports the verdict but leaves
      // persistence to us — flag it so the request banner shows and survives a
      // refresh.
      const isRequest = !isGroup && m.isRequest;
      const serverUrl = getServerUrl(accountId);
      const newConv: Conversation = {
        id: conversationId,
        title: isGroup ? "Group" : m.senderDid,
        accountId,
        serverUrl,
        recipientDid: isGroup ? undefined : m.senderDid,
        groupId: m.groupId ?? undefined,
        lastMessage: previewText,
        lastMessageDate: m.sentAtMs ?? Date.now(),
        lastMessageKind: 0,
        isGroup,
        isRequest,
        isBlocked: false,
        // First message in a brand-new (unopened) conversation → 1 unread. (A5)
        unreadCount: 1,
      };
      pendingConversations.add(conversationId);
      setStore("conversations", (prev) => [newConv, ...prev]);
      if (isRequest) {
        void serviceFor(accountId)
          .setPendingRequest(m.senderDid, true)
          .catch((e: unknown) => {
            console.warn("setPendingRequest failed:", e);
          });
      }
    }
  }

  // Apply a batch of delivery-status receipts (iOS `applyDeliveryStatusUpdates`).
  // Receipts only ever move a message forward; `failed` applies only from a
  // non-terminal state (see `deliveryRank`).
  function applyDeliveryStatusUpdates(
    updates: Extract<IncomingEvent, { type: "receiptUpdate" }>["update"][]
  ) {
    for (const update of updates) {
      const msgs = store.messagesByConversation[update.conversationId];
      if (!msgs) continue;
      const incoming = (
        update.deliveryStatus >= 0 && update.deliveryStatus <= 4
          ? update.deliveryStatus
          : DeliveryStatus.sent
      ) as DeliveryStatus;
      setStore(
        "messagesByConversation",
        update.conversationId,
        msgs.map((m) => {
          if (m.sentAtMs !== update.sentAtMs) return m;
          if (incoming === DeliveryStatus.failed) {
            // Only apply `failed` when the message is still non-terminal
            // (sending/sent). A delivered or read message is never downgraded
            // to failed by a stale or out-of-order receipt.
            if (deliveryRank(m.deliveryStatus) <= deliveryRank(DeliveryStatus.sent)) {
              return { ...m, deliveryStatus: DeliveryStatus.failed };
            }
            return m;
          }
          // For normal forward states, only advance — never go backwards.
          if (deliveryRank(incoming) > deliveryRank(m.deliveryStatus)) {
            return { ...m, deliveryStatus: incoming };
          }
          return m;
        })
      );
    }
  }

  // Apply an inbound edit (iOS `applyInboundEdit`).
  // Matches the target by (senderAccountId, sentAtMs) — identical to iOS
  // applyInboundEdit. T66 (also match on serverId, to disambiguate two messages
  // that share a millisecond) is blocked: the messageEdited/messageDeleted wire
  // events carry no server_id (only conversation_id, author_did, sent_at_ms), so
  // there is nothing to match against. Closing this needs server_id added to
  // those events across the protocol + all platforms — a contract change to
  // raise with the maintainer, not a desktop-only edit.
  function applyInboundEdit(
    cid: string,
    authorDid: string,
    sentAtMs: number,
    newBody: string,
    editedAtMs: number
  ) {
    if (cid && store.messagesByConversation[cid]) {
      setStore("messagesByConversation", cid, (prev) =>
        (prev ?? []).map((m) =>
          m.senderAccountId === authorDid && m.sentAtMs === sentAtMs
            ? {
                ...m,
                body: newBody,
                editedAtMs,
                editCount: m.editCount + 1,
              }
            : m
        )
      );
      // Update conversation preview if the edited message was the most recent.
      const convIdx = store.conversations.findIndex((c) => c.id === cid);
      if (
        convIdx >= 0 &&
        store.conversations[convIdx]?.lastMessageDate === sentAtMs
      ) {
        const previewText =
          newBody.length > 100 ? newBody.slice(0, 100) + "..." : newBody;
        setStore("conversations", convIdx, "lastMessage", previewText);
      }
    } else {
      // Messages not loaded or no conversation_id — reload to pick up the edit.
      void reloadConversations();
    }
  }

  // Apply an inbound delete tombstone (iOS `applyInboundDelete`).
  function applyInboundDelete(cid: string, authorDid: string, sentAtMs: number) {
    if (cid && store.messagesByConversation[cid]) {
      setStore("messagesByConversation", cid, (prev) =>
        (prev ?? []).map((m) =>
          m.senderAccountId === authorDid && m.sentAtMs === sentAtMs
            ? { ...m, isDeleted: true }
            : m
        )
      );
      // A deleted message drops its reactions too.
      clearReactionsForMessage(cid, authorDid, sentAtMs);
      // Update conversation preview if the deleted message was the most recent.
      const convIdx = store.conversations.findIndex((c) => c.id === cid);
      if (
        convIdx >= 0 &&
        store.conversations[convIdx]?.lastMessageDate === sentAtMs
      ) {
        setStore("conversations", convIdx, "lastMessage", "[deleted]");
      }
    } else {
      // Messages not loaded or no conversation_id — reload to pick up the tombstone.
      void reloadConversations();
    }
  }

  // Apply an inbound reaction add/remove (iOS `applyInboundReaction`). Replaces
  // any prior reaction by the same reactor on the target message, then re-adds
  // it unless this was a removal.
  function applyInboundReaction(
    cid: string,
    targetAuthor: string,
    targetSentAtMs: number,
    reactorDid: string,
    emoji: string,
    removed: boolean
  ) {
    const current = store.reactionsByConversation[cid] ?? [];
    const withoutReactor = current.filter(
      (x) =>
        !(
          x.targetAuthor === targetAuthor &&
          x.targetSentAtMs === targetSentAtMs &&
          x.reactorDid === reactorDid
        )
    );
    const next = removed
      ? withoutReactor
      : [
          ...withoutReactor,
          {
            conversationId: cid,
            targetAuthor,
            targetSentAtMs,
            reactorDid,
            emoji,
            reactedAtMs: Date.now(),
          },
        ];
    setStore("reactionsByConversation", cid, next);
  }

  function loopHandle(accountId: string): LoopHandle {
    let h = loops.get(accountId);
    if (!h) {
      h = { eventRunning: false, connRunning: false };
      loops.set(accountId, h);
    }
    return h;
  }

  // Per-account event loop. `nextEvents()` blocks until decrypted events arrive
  // for THIS account (WebSocket push via app-core's channel), then drains the
  // batch attributed to it. The Tauri command is async — it parks on the tokio
  // runtime, so the JS event loop stays responsive. Idempotent: guarded by the
  // per-account handle so a second startPollingFor (restore + add-account both
  // call it) never doubles the loop and processes every event twice.
  function startEventLoopFor(accountId: string) {
    if (!accountId) return;
    const h = loopHandle(accountId);
    if (h.eventRunning) return;
    h.eventRunning = true;

    const svc = serviceFor(accountId);
    const loop = async () => {
      if (!h.eventRunning) return;
      try {
        const events = await svc.nextEvents();
        // A poll in flight when the account was torn down (logout / leave /
        // delete) still resolves here; re-check before applying so we never
        // attribute events to a removed account (mirrors iOS's post-await
        // `guard !Task.isCancelled`). The old single-loop code relied on
        // getSoleAccountId() returning null for this; the per-account loops
        // need the explicit check.
        if (!h.eventRunning) return;
        handleIncomingEvents(events, accountId);
        if (h.eventRunning) void loop();
      } catch {
        if (h.eventRunning) {
          h.eventTimeout = setTimeout(() => void loop(), 1000);
        }
      }
    };
    void loop();
  }

  // Per-account connection loop (mirrors iOS `connectionStateLoop`): seed from
  // the current snapshot, then block on `waitForConnectionStateChange` and copy
  // each state the core emits into `connectionStates[accountId]`. The Rust core
  // owns reconnection and surfaces its progress as `reconnecting` / `connected`
  // states, so this loop is a thin mirror — connectivity churn flows through as
  // ConnectionState values, NOT as JS-side retries. A throw means the core/
  // session is gone, which is terminal.
  function startConnectionLoopFor(accountId: string) {
    if (!accountId) return;
    const h = loopHandle(accountId);
    if (h.connRunning) return;
    h.connRunning = true;

    const svc = serviceFor(accountId);
    const loop = async (last: ConnectionState) => {
      while (h.connRunning) {
        let next: ConnectionState;
        try {
          next = await svc.waitForConnectionStateChange(last);
        } catch {
          h.connRunning = false;
          break;
        }
        if (!h.connRunning) break;
        last = next;
        setStore("connectionStates", accountId, next);
      }
    };

    void svc
      .connectionState()
      .then((state) => {
        setStore("connectionStates", accountId, state);
        void loop(state);
      })
      .catch(() => {
        h.connRunning = false;
      });
  }

  function startPollingFor(accountId: string) {
    startEventLoopFor(accountId);
    startConnectionLoopFor(accountId);
  }

  // Tear down ONE account's loops (leave / delete / remove). Clears the
  // per-account handle so a later startPollingFor cleanly restarts it.
  function stopPollingFor(accountId: string) {
    const h = loops.get(accountId);
    if (!h) return;
    h.eventRunning = false;
    h.connRunning = false;
    if (h.eventTimeout) {
      clearTimeout(h.eventTimeout);
      h.eventTimeout = undefined;
    }
    loops.delete(accountId);
  }

  // Tear down every account's loops (full logout / provider unmount).
  function stopPolling() {
    for (const accountId of Array.from(loops.keys())) stopPollingFor(accountId);
  }

  onCleanup(stopPolling);

  // ── Derived: aggregate connection state ───────────────────────────────────

  const aggregateConnectionState = createMemo((): ConnectionState => {
    const states = Object.values(store.connectionStates);
    // No connection states yet means no accounts have connected — report
    // disconnected so the UI doesn't show a misleading "connected" indicator
    // before any connection exists.
    if (states.length === 0) return { type: "disconnected" };
    if (states.every((s) => s.type === "connected")) return { type: "connected" };
    for (const s of states) {
      if (s.type === "reconnecting") return s;
    }
    const any = states.find((s) => s.type !== "connected");
    return any ?? { type: "connected" };
  });

  async function validateInvite(token: string): Promise<InviteInfo> {
    return onboardingService().validateInvite(token);
  }

  // Begin the "Sign in to another account" flow: run the onboarding UI on top of
  // the live session without tearing it down. createAccount / recoverFromPhrase /
  // device-link all append + enterApp (which clears isAddingAccount), so the
  // existing accounts and their loops keep running throughout.
  function startAddAccount() {
    setStore("isAddingAccount", true);
  }

  function cancelAddAccount() {
    setStore("isAddingAccount", false);
  }

  const ctx: AppContextValue = {
    store,
    service: onboardingService,
    serviceFor,
    setSelectedTab: (tab) => setStore("selectedTab", tab),
    createAccount,
    restoreAccounts,
    logout,
    serverUrl: () => store.serverUrl,
    setServerUrl,
    closeToTray: () => store.closeToTray,
    setCloseToTray,
    reconnectNow,
    joinServer,
    sendMessage,
    sendGroupMessage,
    sendMessageWithAttachments,
    uploadAttachment,
    downloadAttachment,
    fetchLinkPreview,
    openExternal,
    loadConversationsFromStore,
    loadMessagesFromStore,
    markAllMessagesRead,
    findOrCreateDMConversation,
    aggregateConnectionState,
    unreadCount,
    displayName,
    isBot,
    setPendingInviteToken: (token) => setStore("pendingInviteToken", token),
    validateInvite,
    selectedConversationId,
    selectConversation: (id) => setSelectedConversationId(id),
    reloadConversations,
    groupMetaChange,
    reactionsFor,
    loadReactions,
    toggleReaction,
    editMessage,
    loadMessageRevisions,
    deleteMessage,
    retryMessage,
    createGroupAndOpen,
    joinViaLink,
    leaveGroup,
    acceptRequest,
    deleteRequest,
    reportAndBlock,
    blockContact,
    unblockContact,
    listBlocked,
    getConversationTimer,
    setConversationTimer,
    setAccountDisplayName,
    leaveServer,
    deleteIdentity,
    hasRecovery,
    generateRecoveryPhrase,
    recoverFromPhrase,
    startAddAccount,
    cancelAddAccount,
    deviceLinkShowCode,
    deviceLinkEnterCode,
    deviceLinkComplete,
    deviceLinkCancel,
    linkShowCode,
    linkEnterCode,
    linkSendBundle,
    isDeepLink,
    handleDeepLink,
  };

  return (
    <AppContext.Provider value={ctx}>
      {props.children}
    </AppContext.Provider>
  );
}

export function useApp(): AppContextValue {
  const ctx = useContext(AppContext);
  if (!ctx) throw new Error("useApp must be used inside AppProvider");
  return ctx;
}
