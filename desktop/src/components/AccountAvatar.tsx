import { initials, avatarColorIndex } from "../lib/format";
import "./AccountAvatar.css";

interface Props {
  name: string;
  did: string;
  // Bots render in a hexagon (docs/54). Own-account avatars pass false/omit;
  // ContactAvatar resolves it reactively for peers.
  isBot?: boolean;
}

export default function AccountAvatar(props: Props) {
  return (
    <div class={`account-avatar avatar-c${avatarColorIndex(props.did)}${props.isBot ? " bot" : ""}`}>
      {initials(props.name)}
    </div>
  );
}
