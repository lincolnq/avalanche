import {
  createContext,
  createSignal,
  onCleanup,
  useContext,
  type JSX,
} from "solid-js";
import { createStore, produce } from "solid-js/store";
import { load as loadStore } from "@tauri-apps/plugin-store";
import { listen } from "@tauri-apps/api/event";
import type { Account, InviteInfo, ServerInfo } from "../models";
import { displayHost } from "../lib/format";
import { ServiceMode } from "../services/AvalancheService";
import type { AppContextValue, AppStore, PersistedAccount, SessionGuards } from "./types";
import { createServices } from "./createServices";
import { createConversations } from "./createConversations";
import { createMessaging } from "./createMessaging";
import { createGroupsAndSafety } from "./createGroupsAndSafety";
import { createEventLoops } from "./createEventLoops";

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

  // ── Account lifecycle ─────────────────────────────────────────────────────

  // Shared completion step for every onboarding path: resets the conversation
  // load guard, loads conversations, starts event/connection loops, and clears
  // the onboarding flag.  All three paths (createAccount, restoreAccounts,
  // joinServer) must call this — never inline the steps individually.
  function enterApp() {
    guards.loadedConversations.value = false;
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
    // Drop every account's service and rotate a fresh onboarding service so
    // mock state can't bleed into the next session (see createServices.resetAll).
    services.resetAll();
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
    guards.loadedConversations.value = false;
    guards.loadedMessages.clear();
    guards.loadedReactions.clear();
    guards.pendingConversations.clear();
    // Reset the reactive display-name / is-bot caches so components get a
    // reactive update on logout/mode-switch.
    resetCaches();
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
    services.remove(accountId);
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
