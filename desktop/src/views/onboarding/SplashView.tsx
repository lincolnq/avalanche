import { FiLink } from "solid-icons/fi";
import wordmarkUrl from "../../assets/wordmark.svg";
import "./SplashView.css";

interface SplashViewProps {
  onEnterLink: () => void;
  onRecover: () => void;
  onLinkDevice: () => void;
}

/**
 * Welcome screen. Mirrors iOS SplashView: the wordmark, the "Encrypted
 * organizing" tagline, invite entry as the primary action, and "Recover
 * account" / "Link to an existing device" as plain links. Desktop has no QR
 * scanner (by design, docs/62), so the invite link is the way in; the
 * "Already have an account?" divider is Desktop's own.
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
        <button class="btn-primary splash-primary" onClick={props.onEnterLink}>
          <FiLink size={16} aria-hidden="true" />
          Enter Invite Link
        </button>

        <div class="splash-divider" data-tauri-drag-region>
          <span>Already have an account?</span>
        </div>

        <button class="splash-link" onClick={props.onRecover}>
          Recover account
        </button>
        <button class="splash-link" onClick={props.onLinkDevice}>
          Link to an existing device
        </button>
      </div>
    </div>
  );
}
