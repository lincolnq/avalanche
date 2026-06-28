import { createSignal } from "solid-js";
import { FiArrowLeft, FiUser } from "solid-icons/fi";
import type { InviteInfo } from "../../models/InviteToken";
import "./NewAccountView.css";

interface Props {
  inviteInfo: InviteInfo;
  showRecoverLink: boolean;
  onContinue: (displayName: string) => void;
  onRecover?: () => void;
  onBack?: () => void;
}

export default function NewAccountView(props: Props) {
  const [displayName, setDisplayName] = createSignal("");

  function handleContinue() {
    const name = displayName().trim();
    if (!name) return;
    // Account creation happens after the recovery-phrase step (the phrase seed
    // is the signup credential); this just carries the chosen name forward.
    props.onContinue(name);
  }

  return (
    <div class="new-account">
        <div class="na-title">New Identity</div>
        <div class="na-subtitle">on {props.inviteInfo.serverName}</div>
        <div class="na-avatar"><FiUser size={32} /></div>
        <input
          class="text-input na-input"
          type="text"
          placeholder="Your name"
          value={displayName()}
          onInput={(e) => setDisplayName(e.currentTarget.value)}
          onKeyDown={(e) => { if (e.key === "Enter") handleContinue(); }}
          autofocus
        />
        <button
          class="btn-primary na-btn"
          disabled={!displayName().trim()}
          onClick={handleContinue}
        >
          Continue
        </button>
        {props.showRecoverLink && props.onRecover && (
          <button class="back-btn na-recover" onClick={props.onRecover}>
            Recover an existing identity
          </button>
        )}
        {props.onBack && (
          <button class="back-btn na-back" onClick={props.onBack}><FiArrowLeft size={14} />Back</button>
        )}
    </div>
  );
}
