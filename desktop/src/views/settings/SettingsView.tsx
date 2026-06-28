import { createSignal, Match, Show, Switch } from "solid-js";
import { useNavigate } from "@solidjs/router";
import { FiArrowLeft, FiUser, FiUsers, FiSlash, FiTool, FiChevronRight } from "solid-icons/fi";
import { useApp } from "../../state/AppContext";
import AccountAvatar from "../../components/AccountAvatar";
import AccountsView from "./AccountsView";
import ServerDetailView from "./ServerDetailView";
import IdentityDetailView from "./IdentityDetailView";
import BlockedContactsView from "./BlockedContactsView";
import DevSettingsView from "./DevSettingsView";
import type { Account, ServerInfo } from "../../models";
import "./SettingsView.css";

type Screen =
  | { name: "hub" }
  | { name: "accounts" }
  | { name: "identity"; account: Account }
  | { name: "server"; account: Account; server: ServerInfo }
  | { name: "dev" };

/**
 * Settings root hub (mirrors the role of iOS AccountsView as the settings
 * entry). Drives sub-screens through a back-stack — the same pattern as
 * OnboardingFlow — rather than router routes, so the whole hub lives behind the
 * single /settings route. Blocked contacts render as a modal overlay.
 */
export default function SettingsView() {
  const { store } = useApp();
  const navigate = useNavigate();

  const [stack, setStack] = createSignal<Screen[]>([{ name: "hub" }]);
  const [showBlocked, setShowBlocked] = createSignal(false);

  const current = () => stack()[stack().length - 1];
  const push = (s: Screen) => setStack((prev) => [...prev, s]);
  const pop = () => setStack((prev) => (prev.length > 1 ? prev.slice(0, -1) : prev));

  const soleAccount = () => store.accounts[0] as Account | undefined;

  const identityScreen = () =>
    current().name === "identity" ? (current() as Extract<Screen, { name: "identity" }>) : null;
  const serverScreen = () =>
    current().name === "server" ? (current() as Extract<Screen, { name: "server" }>) : null;

  return (
    <Switch>
      <Match when={current().name === "hub"}>
        <div class="settings-hub">
          <header class="settings-subheader">
            <button class="back-btn" onClick={() => navigate("/chats")}>
              <FiArrowLeft size={14} />Back
            </button>
            <h1>Settings</h1>
          </header>

          <div class="settings-hub-body scrollbar-thin">
            <Show when={soleAccount()}>
              {(account) => (
                <button class="settings-profile-row" onClick={() => push({ name: "identity", account: account() })}>
                  <AccountAvatar name={account().displayName} did={account().id} />
                  <div class="settings-profile-info">
                    <span class="settings-profile-name">{account().displayName}</span>
                    <span class="settings-profile-sub">View profile &amp; identity</span>
                  </div>
                  <FiChevronRight size={18} class="settings-row-chevron" />
                </button>
              )}
            </Show>

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

            <Show when={!soleAccount()}>
              <p class="settings-empty"><FiUser size={14} /> No account signed in.</p>
            </Show>
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
        {(s) => <IdentityDetailView account={s().account} onBack={pop} />}
      </Match>

      <Match when={serverScreen()}>
        {(s) => <ServerDetailView account={s().account} server={s().server} onBack={pop} />}
      </Match>

      <Match when={current().name === "dev"}>
        <DevSettingsView onBack={pop} />
      </Match>
    </Switch>
  );
}
