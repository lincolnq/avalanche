import type { SetStoreFunction } from "solid-js/store";
import type { Account, ServerInfo } from "../models";
import { displayHost } from "../lib/format";
import type { Services } from "./createServices";
import type { AppContextValue, AppStore, PersistedAccount } from "./types";

export interface DeviceLinkDeps {
  store: AppStore;
  setStore: SetStoreFunction<AppStore>;
  onboardingService: Services["onboardingService"];
  serviceFor: Services["serviceFor"];
  registerAccountService: Services["registerAccountService"];
  enterApp: () => void;
  addPersistedAccount: (pa: PersistedAccount) => Promise<void>;
}

// Device linking (T71), both sides. New-device side is account-less
// (onboarding); existing-device side is per-account. deviceLinkComplete
// finishes an account add through the same enterApp/persist contract as
// createAccount (injected from createAccounts).
export type DeviceLink = Pick<
  AppContextValue,
  | "deviceLinkShowCode"
  | "deviceLinkEnterCode"
  | "deviceLinkComplete"
  | "deviceLinkCancel"
  | "linkShowCode"
  | "linkEnterCode"
  | "linkSendBundle"
>;

export function createDeviceLink(deps: DeviceLinkDeps): DeviceLink {
  const {
    store,
    setStore,
    onboardingService,
    serviceFor,
    registerAccountService,
    enterApp,
    addPersistedAccount,
  } = deps;

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

  return {
    deviceLinkShowCode,
    deviceLinkEnterCode,
    deviceLinkComplete,
    deviceLinkCancel,
    linkShowCode,
    linkEnterCode,
    linkSendBundle,
  };
}
