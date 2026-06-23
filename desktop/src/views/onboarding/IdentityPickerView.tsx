import { For, Show } from "solid-js";
import { useApp } from "../../state/AppContext";
import type { InviteInfo } from "../../models/InviteToken";
import type { Account } from "../../models/Account";
import { initials } from "../../lib/format";
import "./IdentityPickerView.css";

interface Props {
  inviteInfo: InviteInfo;
  onSelectAccount: (account: Account) => void;
  onNewIdentity: () => void;
  onBack?: () => void;
}

export default function IdentityPickerView(props: Props) {
  const { store } = useApp();

  return (
    <div class="identity-picker">
        <div class="ip-header">Choose Identity</div>
        <div class="ip-subtitle">Join {props.inviteInfo.serverName} as…</div>

        <Show when={store.accounts.length > 0}>
          <div class="ip-section-label">Existing identities</div>
          <div class="ip-list">
            <For each={store.accounts}>
              {(account) => (
                <div
                  class="ip-account-row"
                  onClick={() => props.onSelectAccount(account)}
                >
                  <div class="ip-avatar">{initials(account.displayName)}</div>
                  <div class="ip-account-info">
                    <div class="ip-account-name">{account.displayName}</div>
                    <div class="ip-account-servers">
                      {account.servers.map((s) => s.name).join(", ")}
                    </div>
                  </div>
                  <span class="ip-chevron">›</span>
                </div>
              )}
            </For>
          </div>
        </Show>

        <div class="ip-section-label">New</div>
        <div class="ip-list">
          <div class="ip-action-row ip-action-new" onClick={props.onNewIdentity}>
            <span>＋</span>
            <span>Create a new identity</span>
            <span class="ip-chevron-push">›</span>
          </div>
        </div>

        {props.onBack && (
          <button class="back-btn ip-back" onClick={props.onBack}>← Back</button>
        )}
    </div>
  );
}
