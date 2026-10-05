import { Show } from "solid-js";
import { initials, avatarColorIndex } from "../lib/format";
import { useApp } from "../state/AppContext";
import "./AccountAvatar.css";

interface Props {
  name: string;
  did: string;
  // Bots render in a hexagon (docs/54). Own-account avatars pass false/omit;
  // ContactAvatar resolves it reactively for peers.
  isBot?: boolean;
}

export default function AccountAvatar(props: Props) {
  const app = useApp();
  // Own account: its photo from the local store (docs/55), else initials.
  const photo = () => app.avatarUrl(props.did, props.did);
  return (
    <div
      class={`account-avatar avatar-c${avatarColorIndex(props.did)}${props.isBot ? " bot" : ""}`}
      classList={{ "has-photo": !!photo() }}
    >
      <Show when={photo()} fallback={initials(props.name)}>
        {(url) => <img class="account-avatar-img" src={url()} alt="" draggable={false} />}
      </Show>
    </div>
  );
}
