import { onMount } from "solid-js";
import { useApp } from "../../state/AppContext";
import "./SplashView.css";

interface SplashViewProps {
  onEnterLink: () => void;
  onRecover: () => void;
  onLinkDevice: () => void;
}

export default function SplashView(props: SplashViewProps) {
  const { restoreAccounts } = useApp();

  onMount(async () => {
    await restoreAccounts();
  });

  return (
    // The whole splash is a drag region (no title bar; traffic lights overlay
    // the top-left). Buttons are children without the attribute, so they still
    // click; the wordmark/tagline carry it so dragging by them works too.
    <div class="splash" data-tauri-drag-region>
      <div class="splash-wordmark" data-tauri-drag-region>Avalanche</div>
      <div class="splash-tagline" data-tauri-drag-region>Secure messaging for organizers</div>
      <div class="splash-actions">
        <button class="btn-primary splash-btn" onClick={props.onEnterLink}>
          Enter Invite Link
        </button>
        <button class="splash-recover-link" onClick={props.onLinkDevice}>
          Link to another device
        </button>
        <button class="splash-recover-link" onClick={props.onRecover}>
          Recover an identity
        </button>
      </div>
    </div>
  );
}
