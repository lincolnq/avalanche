import {
  createContext,
  createSignal,
  onCleanup,
  useContext,
  type JSX,
} from "solid-js";
import { createStore } from "solid-js/store";
import { listen } from "@tauri-apps/api/event";
import { ServiceMode } from "../services/AvalancheService";
import type { AppContextValue, AppStore, SessionGuards } from "./types";
import { createServices } from "./createServices";
import { createConversations } from "./createConversations";
import { createMessaging } from "./createMessaging";
import { createGroupsAndSafety } from "./createGroupsAndSafety";
import { createEventLoops } from "./createEventLoops";
import { createAccounts } from "./createAccounts";
import { createDeviceLink } from "./createDeviceLink";

const AppContext = createContext<AppContextValue | undefined>(undefined);

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

  // Per-account service resolution (see createServices.ts). Destructured so the
  // hot call sites (`serviceFor(...)`, `onboardingService()`) read unchanged.
  const services = createServices({ store });
  const { onboardingService, serviceFor, registerAccountService } = services;

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

  // Load-once / lifecycle guards shared across the state modules (see the
  // SessionGuards doc in ./types.ts).
  const guards: SessionGuards = {
    loadedConversations: { value: false },
    loadedMessages: new Set(),
    loadedReactions: new Set(),
    pendingConversations: new Set(),
  };

  // Conversation list, name/bot caches, and deep-link routing (see
  // createConversations.ts). Destructured so call sites read unchanged.
  const conversations = createConversations({
    store,
    setStore,
    serviceFor,
    guards,
    setSelectedConversationId,
  });
  const {
    loadConversationsFromStore,
    reloadConversations,
    findOrCreateDMConversation,
    displayName,
    isBot,
    isDeepLink,
    handleDeepLink,
    accountIdForConversation,
    getServerUrl,
    findOrCreateGroupConversation,
    cachedDisplayName,
    resetCaches,
  } = conversations;

  // Message timelines, optimistic send, read state, and message actions (see
  // createMessaging.ts). Destructured so call sites read unchanged.
  const messaging = createMessaging({
    store,
    setStore,
    serviceFor,
    onboardingService,
    guards,
    accountIdForConversation,
  });
  const {
    sendMessage,
    sendGroupMessage,
    sendMessageWithAttachments,
    uploadAttachment,
    downloadAttachment,
    fetchLinkPreview,
    openExternal,
    loadMessagesFromStore,
    markAllMessagesRead,
    unreadCount,
    reactionsFor,
    loadReactions,
    toggleReaction,
    editMessage,
    loadMessageRevisions,
    deleteMessage,
    retryMessage,
    reloadMessagesIfLoaded,
    clearReactionsForMessage,
  } = messaging;

  // Group create/join/leave and safety/timers (see createGroupsAndSafety.ts).
  const groupsAndSafety = createGroupsAndSafety({
    store,
    setStore,
    serviceFor,
    guards,
    reloadConversations,
    findOrCreateGroupConversation,
    selectedConversationId,
    setSelectedConversationId,
  });
  const {
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
  } = groupsAndSafety;

  // Per-account event + connection loops, inbound-event handlers, native
  // notifications, and the aggregate connection state (see createEventLoops.ts).
  // Registers the loops' onCleanup; must be called synchronously here.
  const eventLoops = createEventLoops({
    store,
    setStore,
    serviceFor,
    guards,
    reloadConversations,
    getServerUrl,
    cachedDisplayName,
    reloadMessagesIfLoaded,
    clearReactionsForMessage,
    selectedConversationId,
    setGroupMetaChange,
  });
  const {
    startPollingFor,
    stopPollingFor,
    stopPolling,
    aggregateConnectionState,
    reconnectNow,
  } = eventLoops;

  // Account lifecycle, avalanche.json persistence, and settings (see
  // createAccounts.ts). Destructured so call sites read unchanged.
  const accounts = createAccounts({
    store,
    setStore,
    services,
    guards,
    loadConversationsFromStore,
    reloadConversations,
    resetCaches,
    startPollingFor,
    stopPollingFor,
    stopPolling,
    setSelectedConversationId,
  });
  const {
    createAccount,
    restoreAccounts,
    logout,
    setServerUrl,
    setCloseToTray,
    joinServer,
    setAccountDisplayName,
    leaveServer,
    deleteIdentity,
    hasRecovery,
    generateRecoveryPhrase,
    recoverFromPhrase,
    validateInvite,
    startAddAccount,
    cancelAddAccount,
    enterApp,
    addPersistedAccount,
  } = accounts;

  // Device linking, both sides (see createDeviceLink.ts).
  const deviceLink = createDeviceLink({
    store,
    setStore,
    onboardingService,
    serviceFor,
    registerAccountService,
    enterApp,
    addPersistedAccount,
  });
  const {
    deviceLinkShowCode,
    deviceLinkEnterCode,
    deviceLinkComplete,
    deviceLinkCancel,
    linkShowCode,
    linkEnterCode,
    linkSendBundle,
  } = deviceLink;

  // ── Deep-link listener ────────────────────────────────────────────────────
  // Single consumer of `avalanche-deeplink` (emitted by the Rust deep-link
  // plugin, see src-tauri/src/lib.rs). OnboardingFlow's pendingInviteToken
  // effect still drives onboarding navigation for invite tokens.
  let deeplinkUnlisten: (() => void) | undefined;
  listen<string>("avalanche-deeplink", (ev) => handleDeepLink(ev.payload))
    .then((un) => { deeplinkUnlisten = un; })
    .catch(() => { /* Tauri event API unavailable (browser/test) */ });
  onCleanup(() => deeplinkUnlisten?.());

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
