import { createSignal, For, Match, onMount, Show, Switch } from "solid-js";
import { FiUser, FiUsers, FiSlash, FiTool, FiChevronRight } from "solid-icons/fi";
import { getVersion } from "@tauri-apps/api/app";
import { useApp } from "../../state/AppContext";
import { updateStatus, checkForUpdates, installAndRestart, updatesEnabled } from "../../state/updater";
import AccountAvatar from "../../components/AccountAvatar";
import AccountsView from "./AccountsView";
import ServerDetailView from "./ServerDetailView";
import IdentityDetailView from "./IdentityDetailView";
import BlockedContactsView from "./BlockedContactsView";
import DevSettingsView from "./DevSettingsView";
import LinkDeviceView from "./LinkDeviceView";
import type { Account, ServerInfo } from "../../models";
import { onEscape } from "../../lib/onEscape";
import "./SettingsView.css";

type Screen =
  | { name: "hub" }
  | { name: "accounts" }
  | { name: "identity"; account: Account }
  | { name: "server"; account: Account; server: ServerInfo }
  | { name: "linkDevice"; account: Account }
  | { name: "dev" };

/**
 * Settings root hub (mirrors the role of iOS AccountsView as the settings
 * entry). Drives sub-screens through a back-stack — the same pattern as
 * OnboardingFlow — rather than router routes, so the whole hub lives behind the
 * single /settings route. Blocked contacts render as a modal overlay.
 */
export default function SettingsView() {
  const { store } = useApp();

  const [stack, setStack] = createSignal<Screen[]>([{ name: "hub" }]);
  const [showBlocked, setShowBlocked] = createSignal(false);

  const current = () => stack()[stack().length - 1];
  const push = (s: Screen) => setStack((prev) => [...prev, s]);
  const pop = () => setStack((prev) => (prev.length > 1 ? prev.slice(0, -1) : prev));

  const accounts = () => store.accounts as Account[];

  // About (docs/63): the running version and the update state.
  const [version, setVersion] = createSignal<string | null>(null);
  onMount(() => {
    getVersion().then(setVersion).catch(() => setVersion(null));
  });
  const updateText = () => {
    const s = updateStatus();
    switch (s.kind) {
      case "checking": return "Checking for updates…";
      case "upToDate": return "Up to date";
      case "downloading": return `Downloading ${s.version}${s.percent != null ? ` (${s.percent}%)` : ""}…`;
      case "ready": return `Version ${s.version} is ready`;
      case "installing": return `Installing ${s.version}…`;
      case "error": return s.message;
      default: return updatesEnabled ? "Updates install automatically" : "Updates are off in development builds";
    }
  };

  // Esc backs out of a sub-screen (the same as its header Back); at the hub
  // root there's nowhere to go, so decline and let Esc pass through.
  onEscape(() => {
    if (stack().length <= 1) return false;
    pop();
  });

  const identityScreen = () =>
    current().name === "identity" ? (current() as Extract<Screen, { name: "identity" }>) : null;
  const serverScreen = () =>
    current().name === "server" ? (current() as Extract<Screen, { name: "server" }>) : null;
  const linkDeviceScreen = () =>
    current().name === "linkDevice" ? (current() as Extract<Screen, { name: "linkDevice" }>) : null;

  return (
    <Switch>
      <Match when={current().name === "hub"}>
        <div class="page">
          {/* A top-level sidebar destination: no Back (iOS's Settings tab has
              none either); the rail is the way out. */}
          <header class="page-header" data-tauri-drag-region>
            <h1 data-tauri-drag-region>Settings</h1>
          </header>

          <div class="page-body scrollbar-thin">
          <div class="page-column settings-hub-column">
            {/* One profile row per signed-in identity (shared-inbox model — no
                single "active" account). Each opens its identity detail, where
                Link a device / Leave / Delete live, per-account. */}
            <For each={accounts()}>
              {(account) => (
                <button class="settings-profile-row" onClick={() => push({ name: "identity", account })}>
                  <AccountAvatar name={account.displayName} did={account.id} />
                  <div class="settings-profile-info">
                    <span class="settings-profile-name">{account.displayName}</span>
                    <span class="settings-profile-sub">View profile &amp; identity</span>
                  </div>
                  <FiChevronRight size={18} class="settings-row-chevron" />
                </button>
              )}
            </For>

            <div class="settings-group">
              <button class="settings-row" onClick={() => push({ name: "accounts" })}>
                <FiUsers size={18} /><span>Accounts</span><FiChevronRight size={16} class="settings-row-chevron" />
              </button>
              <button class="settings-row" onClick={() => setShowBlocked(true)}>
                <FiSlash size={18} /><span>Blocked Contacts</span><FiChevronRight size={16} class="settings-row-chevron" />
              </button>
              <button class="settings-row" onClick={() => push({ name: "dev" })}>
                <FiTool size={18} /><span>Developer</span><FiChevronRight size={16} class="settings-row-chevron" />
              </button>
            </div>

            <div class="settings-group settings-about">
              <div class="settings-about-row">
                <div class="settings-about-info">
                  <span class="settings-about-title">
                    Avalanche Desktop{version() ? ` ${version()}` : ""}
                  </span>
                  <span class="settings-about-sub">{updateText()}</span>
                </div>
                <Show
                  when={updateStatus().kind === "ready"}
                  fallback={
                    <button
                      class="btn-secondary settings-about-btn"
                      disabled={!updatesEnabled || ["checking", "downloading", "installing"].includes(updateStatus().kind)}
                      onClick={() => void checkForUpdates()}
                    >
                      Check for updates
                    </button>
                  }
                >
                  <button class="btn-primary settings-about-btn" onClick={() => void installAndRestart()}>
                    Restart to update
                  </button>
                </Show>
              </div>
            </div>

            <Show when={accounts().length === 0}>
              <p class="settings-empty"><FiUser size={14} /> No account signed in.</p>
            </Show>
          </div>
          </div>

          <Show when={showBlocked()}>
            <BlockedContactsView onClose={() => setShowBlocked(false)} />
          </Show>
        </div>
      </Match>

      <Match when={current().name === "accounts"}>
        <AccountsView
          onBack={pop}
          onOpenIdentity={(account) => push({ name: "identity", account })}
          onOpenServer={(account, server) => push({ name: "server", account, server })}
        />
      </Match>

      <Match when={identityScreen()}>
        {(s) => (
          <IdentityDetailView
            account={s().account}
            onBack={pop}
            onLinkDevice={() => push({ name: "linkDevice", account: s().account })}
          />
        )}
      </Match>

      <Match when={serverScreen()}>
        {(s) => <ServerDetailView account={s().account} server={s().server} onBack={pop} />}
      </Match>

      <Match when={linkDeviceScreen()}>
        {(s) => <LinkDeviceView accountId={s().account.id} onBack={pop} />}
      </Match>

      <Match when={current().name === "dev"}>
        <DevSettingsView onBack={pop} />
      </Match>
    </Switch>
  );
}
