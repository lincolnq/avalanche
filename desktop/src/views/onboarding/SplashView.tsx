import { onMount } from "solid-js";
import { FiLink, FiSmartphone, FiKey } from "solid-icons/fi";
import { useApp } from "../../state/AppContext";
import wordmarkUrl from "../../assets/wordmark.svg";
import "./SplashView.css";

interface SplashViewProps {
  onEnterLink: () => void;
  onRecover: () => void;
  onLinkDevice: () => void;
}

/**
 * Welcome screen. Mirrors iOS SplashView: the wordmark, the "Encrypted
 * organizing" tagline, the invite entry as the primary action, and account
 * recovery / device linking as secondary ones. Desktop has no QR scanner (by
 * design, docs/62), so the invite link is the way in.
 */
export default function SplashView(props: SplashViewProps) {
  const { restoreAccounts } = useApp();

  // Accounts are restored at launch (createAccounts); this is a no-op then,
  // but re-checks if the welcome screen is reached some other way.
  onMount(() => void restoreAccounts());

  return (
    // The whole splash is a drag region (no title bar; traffic lights overlay
    // the top-left). Buttons are children without the attribute, so they still
    // click.
    <div class="splash" data-tauri-drag-region>
      <div class="splash-hero" data-tauri-drag-region>
        <img class="splash-wordmark" src={wordmarkUrl} alt="Avalanche" draggable={false} />
        <p class="splash-tagline" data-tauri-drag-region>
          Encrypted organizing
        </p>
      </div>

      <div class="splash-actions">
        <button class="btn-primary splash-primary" onClick={props.onEnterLink}>
          <FiLink size={16} aria-hidden="true" />
          Join with an invite link
        </button>
        <p class="splash-hint" data-tauri-drag-region>
          Paste the invite link an organizer sent you.
        </p>

        <div class="splash-divider" data-tauri-drag-region>
          <span>Already have an account?</span>
        </div>

        <button class="splash-secondary" onClick={props.onLinkDevice}>
          <FiSmartphone size={16} aria-hidden="true" />
          <span class="splash-secondary-text">
            <span class="splash-secondary-title">Link to an existing device</span>
            <span class="splash-secondary-sub">Use your phone to sign in on this computer</span>
          </span>
        </button>
        <button class="splash-secondary" onClick={props.onRecover}>
          <FiKey size={16} aria-hidden="true" />
          <span class="splash-secondary-text">
            <span class="splash-secondary-title">Recover account</span>
            <span class="splash-secondary-sub">Restore it with your 12-word recovery phrase</span>
          </span>
        </button>
      </div>
    </div>
  );
}
