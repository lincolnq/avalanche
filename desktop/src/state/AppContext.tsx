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
import type { Account, Conversation, InviteInfo, ServerInfo } from "../models";
import { displayHost } from "../lib/format";
import { DeliveryStatus, type Message } from "../models/Message";
import { ServiceMode, type AvalancheService, type ConnectionState, type StoredMessageFfi, type ConversationSummaryFfi, type IncomingEvent, type ReactionFfi, type MessageRevisionFfi, type MessageTarget, type JoinResultFfi, type ContactRowFfi } from "../services/AvalancheService";
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
  serviceMode: ServiceMode;
  selectedTab: "chats" | "network";
  conversations: Conversation[];
  messagesByConversation: Record<string, Message[]>;
  reactionsByConversation: Record<string, ReactionFfi[]>;
  connectionStates: Record<string, ConnectionState>;
  pendingInviteToken: string | null;
  serverUrl: string;
}

// ── Context value ─────────────────────────────────────────────────────────────

interface AppContextValue {
  store: AppStore;
  service: () => AvalancheService;
  setSelectedTab: (tab: "chats" | "network") => void;
  createAccount: (
    serverUrl: string,
    serverName: string,
    displayName: string,
    inviteToken: string | null
  ) => Promise<void>;
  restoreAccounts: () => Promise<void>;
  logout: () => void;
  serverUrl: () => string;
  setServerUrl: (url: string) => void;
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
  setPendingInviteToken: (token: string | null) => void;
  validateInvite: (token: string) => Promise<InviteInfo>;

  // Conversation selection (lifted so compose/group flows can open a chat)
  selectedConversationId: () => string | null;
  selectConversation: (id: string | null) => void;

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
    masterKey: number[],
    hostingServerUrl: string,
    password: number[]
  ) => Promise<JoinResultFfi>;

  // Track D — safety + timers
  acceptRequest: (conversation: Conversation) => Promise<void>;
  deleteRequest: (conversation: Conversation) => Promise<void>;
  reportAndBlock: (conversation: Conversation, reason: string) => Promise<void>;
  blockContact: (did: string) => Promise<void>;
  unblockContact: (did: string) => Promise<void>;
  listBlocked: () => Promise<ContactRowFfi[]>;
  getConversationTimer: (conversationId: string) => Promise<number | null>;
  setConversationTimer: (recipientDid: string, expirySecs: number | null) => Promise<void>;
}

const AppContext = createContext<AppContextValue | undefined>(undefined);

function makeService(mode: ServiceMode): AvalancheService {
  return mode === ServiceMode.Mock
    ? new MockAvalancheService()
    : new DevServerAvalancheService();
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
    serviceMode: ServiceMode.DevServer,
    selectedTab: "chats",
    conversations: [],
    messagesByConversation: {},
    reactionsByConversation: {},
    connectionStates: {},
    pendingInviteToken: null,
    serverUrl: "http://localhost:3000",
  });

  const [service, setService] = createSignal<AvalancheService>(
    makeService(ServiceMode.DevServer)
  );

  // Selected conversation — lifted into context so compose/group/join flows
  // can programmatically open a conversation. ChatsView mirrors this signal.
  const [selectedConversationId, setSelectedConversationId] = createSignal<string | null>(null);

  // Reactive display-name cache: reads are tracked by Solid so components
  // re-render when a resolved name arrives.  A separate plain Set tracks
  // in-flight fetches to prevent duplicate IPC calls per DID.
  const [displayNameCache, setDisplayNameCache] = createStore<Record<string, string>>({});
  const displayNamePending: Set<string> = new Set();

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
  // Conversation ids created in-memory (e.g. an incoming welcome DM) that aren't
  // backed by a row in the local DB. loadConversationsFromStore preserves only
  // these across a reload, NOT arbitrary DB-absent entries, which would resurrect
  // conversations the DB intentionally dropped (e.g. left groups).
  //
  // Persistence is the frontend's responsibility; app-core does not auto-persist
  // (the client owns local history). On this branch the incoming-message handler
  // does not call saveMessage yet, so a received-DM conversation is held only
  // here in memory and is lost on app restart. Day 4 wires up that persistence:
  // once the handler saves received messages, the conversation shows up in the DB
  // summaries on the next reload and this set just bridges the brief window until
  // then. The drop-on-DB-appearance path below returns a conversation to the
  // normal DB-driven lifecycle once it is persisted (e.g. once an outgoing reply
  // saves).
  //
  // TODO: persist incoming messages in the receive handler (wired up in day 4;
  // removes the session-only / lost-on-restart gap above).
  const pendingConversations: Set<string> = new Set();

  // Event loop lifecycle
  let eventLoopRunning = false;
  let connLoopRunning = false;
  let eventLoopTimeout: ReturnType<typeof setTimeout> | undefined;

  // ── Helpers ────────────────────────────────────────────────────────────────

  /**
   * The single account whose session this context drives today. Multi-account
   * is not yet implemented on desktop.
   *
   * NOTE: the eventual multi-account model is NOT a "currently active" identity
   * the user switches between — all identities share one inbox (per the mobile
   * design). So this helper is a stopgap, not a forward-compatible abstraction:
   * when multi-account lands, callers that assume a single active account will
   * need to fan out over `store.accounts` / merge per-account state rather than
   * just swap which account this returns.
   */
  function getSoleAccountId(): string {
    // TODO(robustness): return `null` instead of `""` so callers can
    // distinguish "no account" from a valid empty-string DID. An empty
    // string as sentinel could collide with real data in edge cases
    // (stale event loop after logout).
    return store.accounts[0]?.id ?? "";
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
    } catch {}
  })();

  // ── Account lifecycle ─────────────────────────────────────────────────────

  // Shared completion step for every onboarding path: resets the conversation
  // load guard, loads conversations, starts event/connection loops, and clears
  // the onboarding flag.  All three paths (createAccount, restoreAccounts,
  // joinServer) must call this — never inline the steps individually.
  function enterApp() {
    loadedConversations.value = false;
    void loadConversationsFromStore();
    startPolling();
    setStore("isOnboarding", false);
  }

  // Only restore once per session.  SplashView.onMount fires on every
  // back-stack push, so guard against a second concurrent or repeat call.
  let restoring = false;
  let restored = false;

  async function restoreAccounts() {
    if (restoring || restored) return;
    restoring = true;

    try {
      const persisted = await persistedAccounts();
      if (persisted.length === 0) return;

      const svc = service();
      for (const p of persisted) {
        try {
          const result = await svc.login(p.dbPath, "dev-placeholder-key");
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
    inviteToken: string | null
  ) {
    const dbPath = `account-${Math.random().toString(36).slice(2, 10)}.db`;
    const result = await service().createAccount(
      serverUrl,
      dbPath,
      // TODO: replace with real key-derivation when PRF is wired.
      "dev-placeholder-key",
      // TODO(assumption): AppCore::create_account must accept empty PRF output
      // (the desktop no-passkey path).  If it validates non-empty bytes,
      // account creation fails with an opaque backend error.  Verify when
      // T31 wires the real command.
      [],
      displayName,
      inviteToken
    );

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

  function resetSession() {
    // Drop the Rust AppCore handle so the old reconnect task + WS connection
    // die on drop.  The TS-owned polling loop has already been stopped by
    // stopPolling() above, so clear_session doesn't need to cancel any thread.
    service().clearSession().catch(() => {});
    // Block restoreAccounts from re-entering while we clear persisted state.
    // Otherwise SplashView.onMount fires restoreAccounts before persistAccounts([])
    // completes, finding stale accounts and auto-signing-in — undoing the logout.
    restoring = true;
    stopPolling();
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
    // Clear persisted accounts, then release the restore guard so a
    // subsequent manual restore or fresh session can proceed cleanly.
    void persistAccounts([]).finally(() => {
      restoring = false;
      restored = false;
    });
  }

  function logout() {
    resetSession();
    // Fresh service instance so mock state (storedMessages, pendingEvents, etc.)
    // doesn't bleed into the next session.
    setService(makeService(store.serviceMode));
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

  // ── Messaging ─────────────────────────────────────────────────────────────

  async function loadConversationsFromStore() {
    if (loadedConversations.value) return;
    loadedConversations.value = true;

    const summaries = await service().loadConversations().catch(() => [] as ConversationSummaryFfi[]);
    const accountId = getSoleAccountId();
    const serverUrl = getServerUrl(accountId);

    const convs: Conversation[] = summaries.map((s) => {
      const isGroup = s.groupTitle !== null || s.conversationId.startsWith("group-");
      const groupId = s.conversationId.startsWith("group-")
        ? s.conversationId.slice("group-".length)
        : undefined;
      const recipientDid = !isGroup
        ? recipientDidFromConvId(s.conversationId, accountId) ?? undefined
        : undefined;
      const title =
        isGroup
          ? s.groupTitle ?? "Group"
          : displayNameCache[recipientDid ?? ""] ?? recipientDid ?? s.conversationId;

      return {
        id: s.conversationId,
        title,
        accountId,
        serverUrl,
        recipientDid,
        groupId,
        lastMessage: s.lastMessage?.body ?? undefined,
        lastMessageDate: s.lastMessage?.sentAtMs ?? undefined,
        lastMessageKind: s.lastMessage?.kind ?? 0,
        lastMessageMetadata: s.lastMessage?.metadata ?? undefined,
        lastMessageSenderDid: s.lastMessage?.senderDid ?? undefined,
        isGroup,
        isRequest: s.isRequest,
        isBlocked: s.isBlocked,
      };
    });

    const dbIds = new Set(convs.map((c) => c.id));
    // A pending conversation that now appears in the DB is fully persisted —
    // stop tracking it so it follows normal DB-driven lifecycle from here on.
    for (const id of dbIds) pendingConversations.delete(id);
    // Preserve only still-unpersisted in-memory conversations. Other DB-absent
    // entries (e.g. a group the DB dropped after leaving) are intentionally let go.
    const preserved = store.conversations.filter(
      (c) => !dbIds.has(c.id) && pendingConversations.has(c.id)
    );
    const merged = [...convs, ...preserved].sort(
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
    void service()
      .loadMessages(cid)
      .then((rows) => {
        setStore("messagesByConversation", cid, rows.map(messageFromFfi));
      })
      .catch((e: unknown) => {
        console.warn("reloadMessagesIfLoaded failed:", cid, e);
      });
  }

  function loadMessagesFromStore(conversationId: string, _accountId: string) {
    if (loadedMessages.has(conversationId)) return;
    loadedMessages.add(conversationId);

    void service()
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
    transportFn: (sentAtMs: number) => Promise<void>,
    errorMessage: string
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
      expireTimerSecs: 0,
    };

    setStore("messagesByConversation", conversationId, (prev) => [
      ...(prev ?? []),
      optimistic,
    ]);

    // Update conversation preview
    const convIdx = store.conversations.findIndex((c) => c.id === conversationId);
    if (convIdx >= 0) {
      setStore("conversations", convIdx, "lastMessage", text);
      setStore("conversations", convIdx, "lastMessageDate", sentAtMs);
    }

    try {
      await transportFn(sentAtMs);
      setStore("messagesByConversation", conversationId, (msgs) =>
        (msgs ?? []).map((m) =>
          m.id === messageId
            ? { ...m, deliveryStatus: DeliveryStatus.sent }
            : m
        )
      );
      // Best-effort persist — log failures to console so they are
      // visible in DevTools but never crash the send path.
      void service()
        .saveMessage({
          id: messageId,
          conversationId,
          senderDid: senderAccountId,
          body: text,
          sentAtMs,
          editedAtMs: null,
          readAtMs: sentAtMs,
          deliveryStatus: DeliveryStatus.sent,
          editCount: 0,
          deleted: false,
          kind: 0,
          metadata: null,
          expireTimerSecs: 0,
          expireAtMs: null,
        })
        .catch((err: unknown) => {
          console.warn("saveMessage failed:", err);
        });
    } catch {
      setStore("messagesByConversation", conversationId, (msgs) =>
        (msgs ?? []).map((m) =>
          m.id === messageId
            ? { ...m, deliveryStatus: DeliveryStatus.failed }
            : m
        )
      );
      throw new Error(errorMessage);
    }
  }

  async function sendMessage(
    conversationId: string,
    text: string,
    recipientDid: string,
    senderAccountId: string
  ) {
    await sendOptimistic(
      conversationId,
      text,
      senderAccountId,
      (sentAtMs) => service().sendDm(recipientDid, Array.from(new TextEncoder().encode(text)), sentAtMs),
      "Send failed"
    );
  }

  async function sendGroupMessage(conversation: Conversation, text: string) {
    if (!conversation.groupId) return;
    await sendOptimistic(
      conversation.id,
      text,
      conversation.accountId,
      (sentAtMs) => service().sendGroupMessage(conversation.groupId!, Array.from(new TextEncoder().encode(text)), sentAtMs),
      "Group send failed"
    );
  }

  function markAllMessagesRead(conversationId: string, accountId: string) {
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
      void service()
        .markMessagesRead(conversationId, now)
        .catch((e: unknown) => {
          console.warn("markMessagesRead failed:", e);
        });
      // Send a read receipt to the DM partner so their bubbles flip to "read".
      // Receipts are 1:1 (a single recipient), so this applies to DMs only —
      // a group has no single recipient. app-core itself refuses to ack reads
      // to an un-accepted sender, so request conversations are handled there.
      const conv = store.conversations.find((c) => c.id === conversationId);
      if (conv && !conv.isGroup && conv.recipientDid && newlyReadSentAt.length > 0) {
        const recipientDid = conv.recipientDid;
        void service()
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
    loadedReactions.add(conversationId);
    void service()
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

    void service()
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
    void service()
      .sendEdit(messageTargetFor(conversation), message.sentAtMs, trimmed, now)
      .catch((e: unknown) => {
        console.warn("sendEdit failed:", e);
      });
  }

  function loadMessageRevisions(
    conversation: Conversation,
    message: Message
  ): Promise<MessageRevisionFfi[]> {
    return service()
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
    void service()
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
    const bytes = Array.from(new TextEncoder().encode(message.body));
    try {
      if (conversation.isGroup && conversation.groupId) {
        await service().sendGroupMessage(conversation.groupId, bytes, sentAtMs);
      } else if (conversation.recipientDid) {
        await service().sendDm(conversation.recipientDid, bytes, sentAtMs);
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
    } catch (e) {
      setStore("messagesByConversation", conversation.id, (prev) =>
        (prev ?? []).map((m) =>
          m.id === message.id
            ? { ...m, deliveryStatus: DeliveryStatus.failed }
            : m
        )
      );
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
    const created = await service().createGroup(title, "", expirySeconds);
    const groupId = created.groupId;
    // Best-effort fan-out: one failed invite must not abort the rest.
    for (const did of recipientDids) {
      try {
        await service().inviteMember(groupId, did, 0);
      } catch (e) {
        console.warn("inviteMember failed for", did, e);
      }
    }
    const conv = findOrCreateGroupConversation(groupId, title, accountId);
    return conv;
  }

  // TODO(track-F): no in-app entry point calls joinViaLink yet. iOS joins a
  // group purely via deep link (no dedicated UI), so the trigger lands with the
  // deep-link plumbing in PR 2. The handler itself is complete.
  async function joinViaLink(
    masterKey: number[],
    hostingServerUrl: string,
    password: number[]
  ): Promise<JoinResultFfi> {
    const result = await service().joinViaLink(masterKey, hostingServerUrl, password);
    await reloadConversations();
    return result;
  }

  // ── Track D: message requests / blocking / timers ──────────────────────────

  async function acceptRequest(conversation: Conversation) {
    if (!conversation.recipientDid) return;
    await service().acceptRequest(conversation.recipientDid).catch((e: unknown) => {
      console.warn("acceptRequest failed:", e);
    });
    await reloadConversations();
  }

  async function deleteRequest(conversation: Conversation) {
    if (!conversation.recipientDid) return;
    await service().deleteRequest(conversation.recipientDid).catch((e: unknown) => {
      console.warn("deleteRequest failed:", e);
    });
    if (selectedConversationId() === conversation.id) setSelectedConversationId(null);
    await reloadConversations();
  }

  async function reportAndBlock(conversation: Conversation, reason: string) {
    if (!conversation.recipientDid) return;
    await service().reportAndBlock(conversation.recipientDid, reason).catch((e: unknown) => {
      console.warn("reportAndBlock failed:", e);
    });
    await reloadConversations();
  }

  async function blockContact(did: string) {
    await service().blockContact(did).catch((e: unknown) => {
      console.warn("blockContact failed:", e);
    });
    await reloadConversations();
  }

  async function unblockContact(did: string) {
    await service().unblockContact(did).catch((e: unknown) => {
      console.warn("unblockContact failed:", e);
    });
    await reloadConversations();
  }

  function listBlocked(): Promise<ContactRowFfi[]> {
    return service().listBlocked().catch((e: unknown) => {
      console.warn("listBlocked failed:", e);
      return [] as ContactRowFfi[];
    });
  }

  function getConversationTimer(conversationId: string): Promise<number | null> {
    return service().getConversationTimer(conversationId).catch((e: unknown) => {
      console.warn("getConversationTimer failed:", e);
      return null;
    });
  }

  async function setConversationTimer(recipientDid: string, expirySecs: number | null) {
    await service().setConversationTimer(recipientDid, expirySecs).catch((e: unknown) => {
      console.warn("setConversationTimer failed:", e);
    });
  }

  function unreadCount(conversation: Conversation): number {
    const msgs = store.messagesByConversation[conversation.id] ?? [];
    return msgs.filter(
      (m) => m.readAtMs === undefined && m.senderAccountId !== conversation.accountId
    ).length;
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
      void service()
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
    void accountId; // suppress lint
    return did;
  }

  // ── Event loop ────────────────────────────────────────────────────────────

  // Drain a batch of decrypted events (mirrors iOS `AppState.eventLoop`,
  // AppState.swift). The switch only *collects*; state is applied once after the
  // loop, so a whole batch triggers at most one conversation reload — this is
  // what kills the interleaving-reload races the old per-case reloads caused.
  // Point mutations that don't benefit from batching (edits, deletes) are
  // applied inline via small named handlers, matching iOS.
  function handleIncomingEvents(events: IncomingEvent[]) {
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
        case "groupInvite":
        case "groupMetadataChanged":
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

    // Apply phase — run once for the whole batch.
    const accountId = getSoleAccountId();
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
    };
    setStore("messagesByConversation", conversationId, (prev) => [
      ...(prev ?? []),
      msg,
    ]);

    // Update conversation preview, or create the conversation in-memory.
    const convIdx = store.conversations.findIndex((c) => c.id === conversationId);
    if (convIdx >= 0) {
      const previewText = body.length > 100 ? body.slice(0, 100) + "…" : body;
      setStore("conversations", convIdx, "lastMessage", previewText);
      setStore(
        "conversations",
        convIdx,
        "lastMessageDate",
        m.sentAtMs ?? Date.now()
      );
    } else {
      // Conversation not in the list yet — create it in-memory and mark it
      // pending so the merge in loadConversationsFromStore preserves it across
      // reloads until its message lands in the DB.
      const isGroup = !!m.groupId;
      const serverUrl = getServerUrl(accountId);
      const newConv: Conversation = {
        id: conversationId,
        title: isGroup ? "Group" : m.senderDid,
        accountId,
        serverUrl,
        recipientDid: isGroup ? undefined : m.senderDid,
        groupId: m.groupId ?? undefined,
        lastMessage: body.length > 100 ? body.slice(0, 100) + "…" : body,
        lastMessageDate: m.sentAtMs ?? Date.now(),
        lastMessageKind: 0,
        isGroup,
        isRequest: false,
        isBlocked: false,
      };
      pendingConversations.add(conversationId);
      setStore("conversations", (prev) => [newConv, ...prev]);
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
  // TODO(robustness): matching solely on senderAccountId+sentAtMs can collide
  // if two messages share the same millisecond timestamp. Additionally match on
  // serverId once echo reconciliation assigns it.
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

  function startEventLoop() {
    if (eventLoopRunning) return;
    eventLoopRunning = true;

    // Unified async polling loop.  `nextEvents()` blocks until decrypted
    // events arrive (WebSocket push via app-core's channel), then drains the
    // batch.  The Tauri command is async — it parks on the tokio runtime, so
    // the JS event loop stays responsive.  Both DevServer and Mock mode use
    // the same pattern; the ordering race is gone because the consumer owns
    // the cadence.
    const loop = async () => {
      if (!eventLoopRunning) return;
      try {
        const events = await service().nextEvents();
        handleIncomingEvents(events);
        if (eventLoopRunning) void loop();
      } catch {
        if (eventLoopRunning) {
          eventLoopTimeout = setTimeout(() => void loop(), 1000);
        }
      }
    };
    void loop();
  }

  // Mirror iOS `connectionStateLoop` (AppState.swift:1427): seed from the
  // current snapshot, then block on `waitForConnectionStateChange` and copy
  // each state the core emits into `connectionStates[accountId]`. The Rust
  // core owns reconnection and surfaces its progress as `reconnecting` /
  // `connected` states, so this loop is a thin mirror — connectivity churn
  // flows through as ConnectionState values, NOT as JS-side retries. A throw
  // means the core/session is gone, which is terminal (we deliberately do not
  // layer a second backoff scheme on top of the core's own reconnect logic).
  function startConnectionLoop() {
    if (connLoopRunning) return;
    const accountId = getSoleAccountId();
    // Guard against empty DID: prevents storing connection state at key ""
    // which would pollute the aggregate with a phantom connection.  This
    // also catches the transient state after resetSession clears accounts
    // but before enterApp re-establishes a session.
    if (!accountId) return;
    connLoopRunning = true;

    const loop = async (last: ConnectionState) => {
      while (connLoopRunning) {
        let next: ConnectionState;
        try {
          next = await service().waitForConnectionStateChange(last);
        } catch {
          connLoopRunning = false;
          break;
        }
        if (!connLoopRunning) break;
        last = next;
        setStore("connectionStates", accountId, next);
      }
    };

    void service()
      .connectionState()
      .then((state) => {
        setStore("connectionStates", accountId, state);
        void loop(state);
      })
      .catch(() => {
        connLoopRunning = false;
      });
  }

  function startPolling() {
    startEventLoop();
    startConnectionLoop();
  }

  function stopPolling() {
    eventLoopRunning = false;
    connLoopRunning = false;
    if (eventLoopTimeout) {
      clearTimeout(eventLoopTimeout);
      eventLoopTimeout = undefined;
    }
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
    return service().validateInvite(token);
  }

  const ctx: AppContextValue = {
    store,
    service,
    setSelectedTab: (tab) => setStore("selectedTab", tab),
    createAccount,
    restoreAccounts,
    logout,
    serverUrl: () => store.serverUrl,
    setServerUrl,
    joinServer,
    sendMessage,
    sendGroupMessage,
    loadConversationsFromStore,
    loadMessagesFromStore,
    markAllMessagesRead,
    findOrCreateDMConversation,
    aggregateConnectionState,
    unreadCount,
    displayName,
    setPendingInviteToken: (token) => setStore("pendingInviteToken", token),
    validateInvite,
    selectedConversationId,
    selectConversation: (id) => setSelectedConversationId(id),
    reactionsFor,
    loadReactions,
    toggleReaction,
    editMessage,
    loadMessageRevisions,
    deleteMessage,
    retryMessage,
    createGroupAndOpen,
    joinViaLink,
    acceptRequest,
    deleteRequest,
    reportAndBlock,
    blockContact,
    unblockContact,
    listBlocked,
    getConversationTimer,
    setConversationTimer,
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
