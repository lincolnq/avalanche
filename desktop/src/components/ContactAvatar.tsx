import { Show } from "solid-js";
import { FiUsers } from "solid-icons/fi";
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
  // Show a group glyph instead of initials (a group invite request, which has
  // no real name or photo yet).
  groupGlyph?: boolean;
  // A group's id: show the group photo (docs/55) instead of a person's.
  groupId?: string;
  // Rendered diameter; default 40.
  size?: "sm" | "md" | "lg";
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
  // Photo if one is set (a group invite request has none to show yet);
  // otherwise initials on the per-DID tint.
  const photo = () => {
    if (props.groupGlyph) return null;
    return props.groupId
      ? app.groupAvatarUrl(props.groupId, props.accountId)
      : app.avatarUrl(props.did, props.accountId);
  };

  return (
    <div
      class={`contact-avatar avatar-c${avatarColorIndex(props.did)} size-${props.size ?? "md"}`}
      classList={{ bot: bot(), "has-photo": !!photo() }}
    >
      <Show
        when={photo()}
        fallback={props.groupGlyph ? <FiUsers aria-hidden="true" /> : initials(props.name) || "?"}
      >
        {(url) => <img class="contact-avatar-img" src={url()} alt="" draggable={false} />}
      </Show>
    </div>
  );
}
