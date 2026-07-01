import { For } from "solid-js";
import { FiArrowLeft, FiChevronRight, FiPlus } from "solid-icons/fi";
import { useApp } from "../../state/AppContext";
import AccountAvatar from "../../components/AccountAvatar";
import type { Account, ServerInfo } from "../../models";
import "./AccountsView.css";

interface Props {
  onBack: () => void;
  onOpenIdentity: (account: Account) => void;
  onOpenServer: (account: Account, server: ServerInfo) => void;
}

/**
 * Accounts list: each identity with its servers, plus "Sign in to another
 * account" (Day-7 multi-account). Mirrors iOS AccountsView, minus "Scan Invite"
 * (QR divergence). Adding an account runs onboarding over the live session
 * (startAddAccount) without tearing down the signed-in identities.
 */
export default function AccountsView(props: Props) {
  const { store, startAddAccount } = useApp();

  const isHome = (account: Account, server: ServerInfo) =>
    account.servers[0]?.id === server.id;

  return (
    <div class="accounts-view">
      <header class="settings-subheader">
        <button class="back-btn" onClick={props.onBack}>
          <FiArrowLeft size={14} />Back
        </button>
        <h1>Accounts</h1>
      </header>

      <div class="accounts-body scrollbar-thin">
        <For each={store.accounts}>
          {(account) => (
            <section class="accounts-card">
              <button class="accounts-identity-row" onClick={() => props.onOpenIdentity(account)}>
                <AccountAvatar name={account.displayName} did={account.id} />
                <span class="accounts-identity-name">{account.displayName}</span>
                <FiChevronRight size={18} class="accounts-chevron" />
              </button>

              <For each={[...account.servers].sort((a, b) => a.name.localeCompare(b.name))}>
                {(server) => (
                  <button class="accounts-server-row" onClick={() => props.onOpenServer(account, server)}>
                    <span class="accounts-server-name">{server.name}</span>
                    {isHome(account, server) && <span class="accounts-home-badge">home</span>}
                    <FiChevronRight size={16} class="accounts-chevron" />
                  </button>
                )}
              </For>
            </section>
          )}
        </For>

        <button class="accounts-add-row" onClick={() => startAddAccount()}>
          <FiPlus size={18} />
          <span>Sign in to another account</span>
        </button>
      </div>
    </div>
  );
}
