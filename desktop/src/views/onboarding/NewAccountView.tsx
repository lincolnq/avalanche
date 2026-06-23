import { createSignal } from "solid-js";
import { useApp } from "../../state/AppContext";
import type { InviteInfo } from "../../models/InviteToken";
import "./NewAccountView.css";

interface Props {
  inviteInfo: InviteInfo;
  showRecoverLink: boolean;
  onBack?: () => void;
}

export default function NewAccountView(props: Props) {
  const { createAccount } = useApp();
  const [displayName, setDisplayName] = createSignal("");
  const [isCreating, setIsCreating] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  async function handleCreate() {
    const name = displayName().trim();
    if (!name || isCreating()) return;
    setError(null);
    setIsCreating(true);
    try {
      await createAccount(
        props.inviteInfo.serverUrl,
        props.inviteInfo.serverName,
        name,
        props.inviteInfo.token
      );
      // createAccount sets isOnboarding = false — App.tsx unmounts the flow.
    } catch (e) {
      setError(e instanceof Error ? e.message : "Account creation failed");
      setIsCreating(false);
    }
  }

  return (
    <div class="new-account">
        <div class="na-title">New Identity</div>
        <div class="na-subtitle">on {props.inviteInfo.serverName}</div>
        <div class="na-avatar">👤</div>
        <input
          class="text-input na-input"
          type="text"
          placeholder="Your name"
          value={displayName()}
          onInput={(e) => setDisplayName(e.currentTarget.value)}
          onKeyDown={(e) => { if (e.key === "Enter") void handleCreate(); }}
          disabled={isCreating()}
          autofocus
        />
        {error() && <div class="na-error">{error()}</div>}
        <button
          class="btn-primary na-btn"
          disabled={!displayName().trim() || isCreating()}
          onClick={() => void handleCreate()}
        >
          {isCreating() && <span class="spinner" />}
          {isCreating() ? "Creating…" : "Create Identity"}
        </button>
        {props.onBack && !isCreating() && (
          <button class="back-btn na-back" onClick={props.onBack}>← Back</button>
        )}
    </div>
  );
}
