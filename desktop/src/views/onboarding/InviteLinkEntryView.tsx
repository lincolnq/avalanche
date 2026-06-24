import { createSignal } from "solid-js";
import { useApp } from "../../state/AppContext";
import type { InviteInfo } from "../../models/InviteToken";
import { useInviteValidation } from "../../lib/useInviteValidation";
import "./InviteLinkEntryView.css";

interface Props {
  onValidated: (info: InviteInfo, token: string) => void;
  onBack?: () => void;
}

export default function InviteLinkEntryView(props: Props) {
  const { validateInvite } = useApp();
  const [linkText, setLinkText] = createSignal("");

  const { error, isValidating, validate: handleValidate } = useInviteValidation(
    validateInvite,
    props.onValidated,
    "Invalid invite link"
  );

  return (
    <div class="invite-entry">
      <div class="ie-title">Enter Invite Link</div>
      <div class="ie-subtitle">Paste your Avalanche invite link or bare token below.</div>
      <input
        class="text-input ie-input"
        type="text"
        placeholder="actnet://... or paste token"
        value={linkText()}
        onInput={(e) => setLinkText(e.currentTarget.value)}
        onKeyDown={(e) => { if (e.key === "Enter") void handleValidate(linkText()); }}
        autofocus
      />
      {error() && <div class="ie-error">{error()}</div>}
      <button
        class="btn-primary ie-btn"
        disabled={!linkText().trim() || isValidating()}
        onClick={() => void handleValidate(linkText())}
      >
        {isValidating() ? "Validating…" : "Continue"}
      </button>
      {props.onBack && (
        <button class="back-btn ie-back" onClick={props.onBack}>← Back</button>
      )}
    </div>
  );
}
