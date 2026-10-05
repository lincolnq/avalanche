import { FiSmartphone } from "solid-icons/fi";
import wordmarkUrl from "../../assets/wordmark.svg";
import "./SplashView.css";

interface SplashViewProps {
  onEnterLink: () => void;
  onRecover: () => void;
  onLinkDevice: () => void;
}

/**
 * Welcome screen. The wordmark and "Encrypted organizing" tagline mirror iOS
 * SplashView, but the emphasis differs: Desktop is a companion to the mobile
 * app, so linking to your phone is the primary action, and joining by invite
 * link or restoring from a recovery phrase sit under "Using Desktop on its
 * own?". (Desktop recovery is phrase-only; phone accounts usually use a
 * passkey, so the label names the phrase.)
 */
export default function SplashView(props: SplashViewProps) {
  // No restore here: saved accounts are opened once at launch (createAccounts).
  // Re-running it on every mount retried a failing open — a ~200 ms key
  // derivation — each time this screen appeared.

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
        <button class="btn-primary splash-primary" onClick={props.onLinkDevice}>
          <FiSmartphone size={16} aria-hidden="true" />
          Link to your phone
        </button>
        <p class="splash-note" data-tauri-drag-region>
          Avalanche Desktop works alongside the mobile app.
        </p>

        <div class="splash-divider" data-tauri-drag-region>
          <span>Using Desktop on its own?</span>
        </div>

        <button class="splash-link" onClick={props.onEnterLink}>
          Join with an invite link
        </button>
        <button class="splash-link" onClick={props.onRecover}>
          Restore from a recovery phrase
        </button>
      </div>
    </div>
  );
}
