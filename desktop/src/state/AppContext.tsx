import {
  createContext,
  createMemo,
  createSignal,
  onCleanup,
  useContext,
  type JSX,
} from "solid-js";
import { createStore, produce } from "solid-js/store";
import { load as loadStore } from "@tauri-apps/plugin-store";
import type { Account, Conversation, ServerInfo } from "../models";
import { groupConversationId } from "../models";
import { DeliveryStatus, type Message } from "../models/Message";
import { ServiceMode, type ActnetService, type ConnectionState, type StoredMessageFfi, type ConversationSummaryFfi, type IncomingEvent } from "../services/ActnetService";
import { MockActnetService } from "../services/MockActnetService";
import { DevServerActnetService } from "../services/DevServerActnetService";

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
  connectionStates: Record<string, ConnectionState>;
  pendingInviteToken: string | null;
}

// ── Context value ─────────────────────────────────────────────────────────────

interface AppContextValue {
  store: AppStore;
  setSelectedTab: (tab: "chats" | "network") => void;
  createAccount: (
    serverUrl: string,
    serverName: string,
    displayName: string,
    inviteToken: string | null
  ) => Promise<void>;
  restoreAccounts: () => Promise<void>;
  logout: () => void;
  switchMode: (mode: ServiceMode) => void;
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
}

const AppContext = createContext<AppContextValue | undefined>(undefined);

function makeService(mode: ServiceMode): ActnetService {
  return mode === ServiceMode.Mock
    ? new MockActnetService()
    : new DevServerActnetService();
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

// ── Provider ──────────────────────────────────────────────────────────────────

export function AppProvider(props: { children: JSX.Element }) {
  const [store, setStore] = createStore<AppStore>({
    accounts: [],
    isOnboarding: true,
    serviceMode: ServiceMode.Mock,
    selectedTab: "chats",
    conversations: [],
    messagesByConversation: {},
    connectionStates: {},
    pendingInviteToken: null,
  });

  const [service, setService] = createSignal<ActnetService>(
    makeService(ServiceMode.Mock)
  );

  // Display name cache — not reactive, just a JS map.
  const displayNameCache: Map<string, string> = new Map();

  // Load-once guards
  const loadedConversations = { value: false };
  const loadedMessages: Set<string> = new Set();

  // Event loop lifecycle
  let eventLoopRunning = false;
  let connLoopRunning = false;

  // ── Helpers ────────────────────────────────────────────────────────────────

  function getServerUrl(accountId: string): string {
    return (
      store.accounts
        .find((a) => a.id === accountId)
        ?.servers[0]?.id ?? ""
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

  async function saveServiceMode(mode: ServiceMode) {
    try {
      const s = await loadStore("avalanche.json");
      await s.set("serviceMode", mode);
      await s.save();
    } catch {}
  }

  // ── Init: read persisted mode on mount ───────────────────────────────────

  void (async () => {
    try {
      const s = await loadStore("avalanche.json");
      const savedMode = await s.get<string>("serviceMode");
      if (
        savedMode === ServiceMode.Mock ||
        savedMode === ServiceMode.DevServer
      ) {
        setStore("serviceMode", savedMode as ServiceMode);
        setService(makeService(savedMode as ServiceMode));
      }
    } catch {}
  })();

  // ── Account lifecycle ─────────────────────────────────────────────────────

  async function restoreAccounts() {
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
            displayHost: (() => {
              try {
                return new URL(srv.url).hostname;
              } catch {
                return srv.name;
              }
            })(),
          })),
        };
        setStore("accounts", (prev) => [...prev, account]);
      } catch {
        // Account login failed — skip; leave persisted for next launch.
      }
    }

    if (store.accounts.length > 0) {
      setStore("isOnboarding", false);
      await loadConversationsFromStore();
      startPolling();
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
      "dev-placeholder-key",
      displayName,
      inviteToken
    );

    const displayHost = (() => {
      try { return new URL(serverUrl).hostname; } catch { return serverName; }
    })();

    const serverInfo: ServerInfo = {
      id: serverUrl,
      name: serverName,
      url: serverUrl,
      displayHost,
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

    loadedConversations.value = false;
    await loadConversationsFromStore();

    setStore("isOnboarding", false);
    startPolling();
  }

  function logout() {
    stopPolling();
    setStore(
      produce((s) => {
        s.accounts = [];
        s.isOnboarding = true;
        s.conversations = [];
        s.messagesByConversation = {};
        s.connectionStates = {};
        s.pendingInviteToken = null;
      })
    );
    loadedConversations.value = false;
    loadedMessages.clear();
    displayNameCache.clear();
    void persistAccounts([]);
  }

  function switchMode(mode: ServiceMode) {
    stopPolling();
    setService(makeService(mode));
    setStore(
      produce((s) => {
        s.serviceMode = mode;
        s.accounts = [];
        s.isOnboarding = true;
        s.conversations = [];
        s.messagesByConversation = {};
        s.connectionStates = {};
        s.pendingInviteToken = null;
      })
    );
    loadedConversations.value = false;
    loadedMessages.clear();
    displayNameCache.clear();
    void persistAccounts([]);
    void saveServiceMode(mode);
  }

  async function joinServer(
    serverUrl: string,
    serverName: string,
    existingAccountId: string
  ) {
    const idx = store.accounts.findIndex((a) => a.id === existingAccountId);
    if (idx >= 0) {
      const displayHost = (() => {
        try { return new URL(serverUrl).hostname; } catch { return serverName; }
      })();
      setStore("accounts", idx, "servers", (prev) => [
        ...prev,
        { id: serverUrl, name: serverName, url: serverUrl, displayHost },
      ]);
    }
    setStore("isOnboarding", false);
  }

  // ── Messaging ─────────────────────────────────────────────────────────────

  async function loadConversationsFromStore() {
    if (loadedConversations.value) return;
    loadedConversations.value = true;

    const summaries = await service().loadConversations().catch(() => [] as ConversationSummaryFfi[]);
    const accountId = store.accounts[0]?.id ?? "";
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
          : displayNameCache.get(recipientDid ?? "") ?? recipientDid ?? s.conversationId;

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

    const sorted = [...convs].sort(
      (a, b) => (b.lastMessageDate ?? 0) - (a.lastMessageDate ?? 0)
    );
    setStore("conversations", sorted);
  }

  function loadMessagesFromStore(conversationId: string, _accountId: string) {
    if (loadedMessages.has(conversationId)) return;
    loadedMessages.add(conversationId);

    void service()
      .loadMessages(conversationId)
      .then((rows) => {
        const messages = rows.map(messageFromFfi);
        setStore("messagesByConversation", conversationId, messages);
      })
      .catch(() => {});
  }

  async function sendMessage(
    conversationId: string,
    text: string,
    recipientDid: string,
    senderAccountId: string
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
      await service().sendDm(recipientDid, text, sentAtMs);
      setStore("messagesByConversation", conversationId, (msgs) =>
        (msgs ?? []).map((m) =>
          m.id === messageId
            ? { ...m, deliveryStatus: DeliveryStatus.sent }
            : m
        )
      );
      // Best-effort persist
      void service().saveMessage({
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
      });
    } catch {
      setStore("messagesByConversation", conversationId, (msgs) =>
        (msgs ?? []).map((m) =>
          m.id === messageId
            ? { ...m, deliveryStatus: DeliveryStatus.failed }
            : m
        )
      );
      throw new Error("Send failed");
    }
  }

  async function sendGroupMessage(conversation: Conversation, text: string) {
    if (!conversation.groupId) return;
    const messageId = crypto.randomUUID();
    const sentAtMs = Date.now();

    const optimistic: Message = {
      id: messageId,
      conversationId: conversation.id,
      senderAccountId: conversation.accountId,
      body: text,
      sentAtMs,
      readAtMs: sentAtMs,
      deliveryStatus: DeliveryStatus.sending,
      editCount: 0,
      isDeleted: false,
      kind: 0,
      expireTimerSecs: 0,
    };

    setStore("messagesByConversation", conversation.id, (prev) => [
      ...(prev ?? []),
      optimistic,
    ]);

    const convIdx = store.conversations.findIndex((c) => c.id === conversation.id);
    if (convIdx >= 0) {
      setStore("conversations", convIdx, "lastMessage", text);
      setStore("conversations", convIdx, "lastMessageDate", sentAtMs);
    }

    try {
      await service().sendGroupMessage(conversation.groupId, text, sentAtMs);
      setStore("messagesByConversation", conversation.id, (msgs) =>
        (msgs ?? []).map((m) =>
          m.id === messageId
            ? { ...m, deliveryStatus: DeliveryStatus.sent }
            : m
        )
      );
    } catch {
      setStore("messagesByConversation", conversation.id, (msgs) =>
        (msgs ?? []).map((m) =>
          m.id === messageId
            ? { ...m, deliveryStatus: DeliveryStatus.failed }
            : m
        )
      );
      throw new Error("Group send failed");
    }
  }

  function markAllMessagesRead(conversationId: string, accountId: string) {
    const msgs = store.messagesByConversation[conversationId];
    if (!msgs) return;
    const now = Date.now();
    let changed = false;
    const updated = msgs.map((m) => {
      if (m.readAtMs === undefined && m.senderAccountId !== accountId) {
        changed = true;
        return { ...m, readAtMs: now };
      }
      return m;
    });
    if (changed) {
      setStore("messagesByConversation", conversationId, updated);
      void service()
        .markMessagesRead(conversationId, now)
        .catch(() => {});
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
    const title = displayNameCache.get(recipientDid) ?? recipientDid;
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

  function unreadCount(conversation: Conversation): number {
    const msgs = store.messagesByConversation[conversation.id] ?? [];
    return msgs.filter(
      (m) => m.readAtMs === undefined && m.senderAccountId !== conversation.accountId
    ).length;
  }

  function displayName(did: string, accountId: string): string {
    const own = store.accounts.find((a) => a.id === did);
    if (own) return own.displayName;
    if (displayNameCache.has(did)) return displayNameCache.get(did)!;
    // Kick off async resolution (best-effort, no await)
    void service()
      .contactDisplayName(did)
      .then((name) => {
        if (name) {
          displayNameCache.set(did, name);
          // Update conversation titles that show the raw DID
          store.conversations.forEach((c, i) => {
            if (c.recipientDid === did && c.title === did) {
              setStore("conversations", i, "title", name);
            }
          });
        }
      })
      .catch(() => {});
    void accountId; // suppress lint
    return did;
  }

  // ── Event loop ────────────────────────────────────────────────────────────

  function handleIncomingEvents(events: IncomingEvent[]) {
    for (const ev of events) {
      switch (ev.type) {
        case "message": {
          const m = ev.msg;
          const msg = messageFromFfi(m);
          setStore("messagesByConversation", m.conversationId, (prev) => [
            ...(prev ?? []),
            msg,
          ]);
          // Update conversation preview
          const convIdx = store.conversations.findIndex(
            (c) => c.id === m.conversationId
          );
          if (convIdx >= 0) {
            setStore("conversations", convIdx, "lastMessage", m.body);
            setStore("conversations", convIdx, "lastMessageDate", m.sentAtMs);
          }
          break;
        }
        case "receiptUpdate": {
          const msgs = store.messagesByConversation[ev.conversationId];
          if (msgs) {
            setStore(
              "messagesByConversation",
              ev.conversationId,
              msgs.map((m) =>
                m.sentAtMs === ev.sentAtMs &&
                ev.deliveryStatus > m.deliveryStatus
                  ? { ...m, deliveryStatus: ev.deliveryStatus as DeliveryStatus }
                  : m
              )
            );
          }
          break;
        }
        case "groupInvite":
        case "groupMetadataChanged":
        case "storageSynced":
          loadedConversations.value = false;
          void loadConversationsFromStore();
          break;
        default:
          break;
      }
    }
  }

  function startEventLoop() {
    eventLoopRunning = true;
    const loop = async () => {
      if (!eventLoopRunning) return;
      try {
        const events = await service().nextEvents();
        handleIncomingEvents(events);
      } catch {
        // service errored or loop was stopped
      }
      if (eventLoopRunning) void loop();
    };
    void loop();
  }

  function startConnectionLoop() {
    connLoopRunning = true;
    const accountId = store.accounts[0]?.id;
    if (!accountId) return;

    const loop = async (last: ConnectionState) => {
      if (!connLoopRunning) return;
      try {
        const next = await service().waitForConnectionStateChange(last);
        setStore("connectionStates", accountId, next);
        if (connLoopRunning) void loop(next);
      } catch {}
    };

    void service()
      .connectionState()
      .then((state) => {
        setStore("connectionStates", accountId, state);
        void loop(state);
      })
      .catch(() => {});
  }

  function startPolling() {
    startEventLoop();
    startConnectionLoop();
  }

  function stopPolling() {
    eventLoopRunning = false;
    connLoopRunning = false;
  }

  onCleanup(stopPolling);

  // ── Derived: aggregate connection state ───────────────────────────────────

  const aggregateConnectionState = createMemo((): ConnectionState => {
    const states = Object.values(store.connectionStates);
    if (states.length === 0) return { type: "connected" };
    if (states.every((s) => s.type === "connected")) return { type: "connected" };
    for (const s of states) {
      if (s.type === "reconnecting") return s;
    }
    const any = states.find((s) => s.type !== "connected");
    return any ?? { type: "connected" };
  });

  const ctx: AppContextValue = {
    store,
    setSelectedTab: (tab) => setStore("selectedTab", tab),
    createAccount,
    restoreAccounts,
    logout,
    switchMode,
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
