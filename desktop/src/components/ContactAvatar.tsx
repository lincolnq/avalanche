import { Show } from "solid-js";
import { initials, avatarColorIndex } from "../lib/format";
import { useApp } from "../state/AppContext";
import "./ContactAvatar.css";

interface Props {
  name: string;
  did: string;
  // The account whose core resolves bot status (per-account contact store).
  accountId: string;
  // Optional override; when omitted, bot status is resolved reactively from the
  // context cache (getAccountInfo).
  isBot?: boolean;
}

/**
 * Avatar for a contact (someone other than the local user). People render in a
 * circle, bots in a hexagon (docs/54 bot presentation) — the frame is the bot
 * signal, applied client-side over whatever the account supplies. Mirrors iOS
 * ContactAvatar + Hexagon.
 */
export default function ContactAvatar(props: Props) {
  const app = useApp();
  const bot = () => props.isBot ?? app.isBot(props.did, props.accountId);

  return (
    <div class={`contact-avatar avatar-c${avatarColorIndex(props.did)}${bot() ? " bot" : ""}`}>
      {initials(props.name) || "?"}
      <Show when={bot()}>
        <span class="contact-avatar-badge" aria-label="Bot" title="Bot">⬡</span>
      </Show>
    </div>
  );
}
