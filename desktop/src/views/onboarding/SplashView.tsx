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
    <div class="splash">
      <div class="splash-wordmark">Avalanche</div>
      <div class="splash-tagline">Secure messaging for organizers</div>
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
