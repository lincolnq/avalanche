import { ServiceMode, type AvalancheService } from "../services/AvalancheService";
import { DevServerAvalancheService } from "../services/DevServerAvalancheService";
import { makeService } from "./helpers";
import type { AppStore } from "./types";

export interface ServicesDeps {
  store: AppStore;
}

// Per-account service resolution. The `services` Map and the rotating
// onboarding instance are private to this factory — every other module resolves
// services exclusively through these functions. `onboardingSvc` is reassigned
// (Mock rotation, resetAll), so consumers must always go through the
// `onboardingService()` getter; a captured instance would go stale.
export interface Services {
  onboardingService: () => AvalancheService;
  serviceFor: (accountId: string) => AvalancheService;
  registerAccountService: (accountId: string) => void;
  remove: (accountId: string) => void;
  resetAll: () => void;
}

export function createServices(deps: ServicesDeps): Services {
  const { store } = deps;

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

  // Drop one removed account's service (leave / delete / remove account).
  function remove(accountId: string) {
    services.delete(accountId);
  }

  // Full logout: drop every account's service and rotate a fresh onboarding
  // service so mock state (storedMessages, pendingEvents, mockDid) can't bleed
  // into the next session — never a module global.
  function resetAll() {
    services.clear();
    onboardingSvc = makeService(store.serviceMode);
  }

  return { onboardingService, serviceFor, registerAccountService, remove, resetAll };
}
