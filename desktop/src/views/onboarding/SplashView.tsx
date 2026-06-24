import { onMount } from "solid-js";
import { useApp } from "../../state/AppContext";
import "./SplashView.css";

interface SplashViewProps {
  onEnterLink: () => void;
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
      </div>
    </div>
  );
}
