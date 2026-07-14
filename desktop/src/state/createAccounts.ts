import { produce, type SetStoreFunction } from "solid-js/store";
import { load as loadStore } from "@tauri-apps/plugin-store";
import type { Account, InviteInfo, ServerInfo } from "../models";
import { displayHost } from "../lib/format";
import type { Services } from "./createServices";
import type { AppContextValue, AppStore, PersistedAccount, SessionGuards } from "./types";

export interface AccountsDeps {
  store: AppStore;
  setStore: SetStoreFunction<AppStore>;
  services: Services;
  guards: SessionGuards;
  loadConversationsFromStore: () => Promise<void>;
  reloadConversations: () => Promise<void>;
  resetCaches: () => void;
  startPollingFor: (accountId: string) => void;
  stopPollingFor: (accountId: string) => void;
  stopPolling: () => void;
  setSelectedConversationId: (id: string | null) => void;
}

// Account lifecycle (create / restore / recover / join / logout / remove),
// avalanche.json persistence, and the server-url / close-to-tray settings.
// Every entry path funnels through `enterApp()` — never inline its steps.
// Pick-typed — see the note in createConversations.ts.
export type Accounts = Pick<
  AppContextValue,
  | "createAccount"
  | "restoreAccounts"
  | "logout"
  | "setServerUrl"
  | "setCloseToTray"
  | "joinServer"
  | "setAccountDisplayName"
  | "leaveServer"
  | "deleteIdentity"
  | "hasRecovery"
  | "generateRecoveryPhrase"
  | "recoverFromPhrase"
  | "validateInvite"
  | "startAddAccount"
  | "cancelAddAccount"
> & {
  // Internal API for the other state modules (device linking completes an
  // account add through the same enterApp/persist contract as createAccount)
  enterApp: () => void;
  addPersistedAccount: (pa: PersistedAccount) => Promise<void>;
};

export function createAccounts(deps: AccountsDeps): Accounts {
  const {
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
  } = deps;
  const { onboardingService, serviceFor, registerAccountService } = services;

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

    // Replace-or-append by DID so registration can't leave a duplicate account
    // id even if this DID is already present (mirrors iOS finishAccountRegistration
    // and the restoreAccounts guard). A duplicate id crashes Android's LazyColumn.
    setStore("accounts", (prev) => [...prev.filter((a) => a.id !== account.id), account]);

    await addPersistedAccount({
      did: result.did,
      displayName: account.displayName,
      dbPath,
      servers: [{ id: serverUrl, name: serverName, url: serverUrl }],
    });

    enterApp();
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
    // Replace-or-append by DID (see createAccount) — the guard above races an
    // await, so append defensively rather than unconditionally.
    setStore("accounts", (prev) => [...prev.filter((a) => a.id !== account.id), account]);
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

  return {
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
  };
}
