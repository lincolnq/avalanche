import { createSignal } from "solid-js";
import { useApp } from "../../state/AppContext";
import type { InviteInfo } from "../../models/InviteToken";
import type { Account } from "../../models/Account";
import { initials } from "../../lib/format";
import "./JoiningServerView.css";

interface Props {
  inviteInfo: InviteInfo;
  account: Account;
  onBack?: () => void;
}

export default function JoiningServerView(props: Props) {
  const { joinServer } = useApp();
  const [isJoining, setIsJoining] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  async function handleJoin() {
    setIsJoining(true);
    setError(null);
    try {
      await joinServer(
        props.inviteInfo.serverUrl,
        props.inviteInfo.serverName,
        props.account.id
      );
      // joinServer sets isOnboarding = false — App.tsx unmounts the flow.
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to join server");
      setIsJoining(false);
    }
  }

  return (
    <div class="joining-server">
        <div class="js-avatar">{initials(props.account.displayName)}</div>
        <div class="js-title">
          Join {props.inviteInfo.serverName} as {props.account.displayName}?
        </div>
        {error() && <div class="js-error">{error()}</div>}
        <button
          class="btn-primary js-btn"
          disabled={isJoining()}
          onClick={() => void handleJoin()}
        >
          {isJoining() && <span class="spinner" />}
          {isJoining() ? "Joining…" : "Join"}
        </button>
        {props.onBack && !isJoining() && (
          <button class="back-btn js-back" onClick={props.onBack}>← Back</button>
        )}
    </div>
  );
}
