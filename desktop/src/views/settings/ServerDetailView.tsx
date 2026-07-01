import { createSignal, Show } from "solid-js";
import { FiArrowLeft, FiHome } from "solid-icons/fi";
import { useApp } from "../../state/AppContext";
import type { Account, ServerInfo } from "../../models";
import "./ServerDetailView.css";

interface Props {
  account: Account;
  server: ServerInfo;
  onBack: () => void;
}

/**
 * Server detail + "Leave this server" (mirrors iOS ServerDetailView). The leave
 * action is offered only for non-home memberships; the home (discovery) server
 * is left via "Delete identity" on the identity screen. Leaving tears the
 * account down locally and returns to onboarding (iOS removeAccountLocally).
 */
export default function ServerDetailView(props: Props) {
  const { leaveServer } = useApp();
  const [confirming, setConfirming] = createSignal(false);
  const [leaving, setLeaving] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const isHome = () => props.account.servers[0]?.id === props.server.id;
  const homeServerName = () => props.account.servers[0]?.name ?? "your home server";

  async function doLeave() {
    setLeaving(true);
    setError(null);
    try {
      await leaveServer(props.account.id);
      // leaveServer drops this account (back to onboarding if it was the last).
    } catch (e) {
      setError(e instanceof Error ? e.message : "Couldn't leave server");
      setLeaving(false);
      setConfirming(false);
    }
  }

  return (
    <div class="server-detail">
      <header class="settings-subheader">
        <button class="back-btn" onClick={props.onBack}>
          <FiArrowLeft size={14} />Back
        </button>
        <h1>Server</h1>
      </header>

      <div class="server-detail-body scrollbar-thin">
        <div class="server-detail-head">
          <h2>{props.server.name}</h2>
          <p class="server-detail-url">{props.server.url}</p>
        </div>

        <Show when={isHome()}>
          <div class="server-home-card">
            <div class="server-home-title">
              <FiHome size={15} />
              <span>Home server for {props.account.displayName}</span>
            </div>
            <p>To change your home server or delete this identity, open the identity screen.</p>
          </div>
        </Show>

        <Show when={error()}>
          <p class="settings-error">{error()}</p>
        </Show>

        <Show when={!isHome()}>
          <Show
            when={confirming()}
            fallback={
              <button class="btn-danger server-leave-btn" onClick={() => setConfirming(true)}>
                Leave this server
              </button>
            }
          >
            <div class="server-confirm">
              <p>
                Leave {props.server.name}? You'll be removed from any groups and
                Projects there. New contacts will reach you at {homeServerName()}.
              </p>
              <div class="server-confirm-actions">
                <button class="btn-secondary" onClick={() => setConfirming(false)} disabled={leaving()}>
                  Cancel
                </button>
                <button class="btn-danger" onClick={() => void doLeave()} disabled={leaving()}>
                  {leaving() ? "Leaving…" : "Leave"}
                </button>
              </div>
            </div>
          </Show>
        </Show>
      </div>
    </div>
  );
}
