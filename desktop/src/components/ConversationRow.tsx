import { Show } from "solid-js";
import { useApp } from "../state/AppContext";
import type { Conversation } from "../models";
import { formatRelative } from "../lib/format";
import { groupEventText } from "../lib/groupEvents";
import ContactAvatar from "./ContactAvatar";
import "./ConversationRow.css";

interface Props {
  conversation: Conversation;
  selected: boolean;
  onSelect: (id: string) => void;
}

export default function ConversationRow(props: Props) {
  const { store, unreadCount, displayName } = useApp();
  // Multi-account: show a small badge with the owning identity's initial so the
  // merged inbox makes clear which account each chat belongs to. Only when more
  // than one account is signed in (mirrors iOS ConversationRow.showAccountIndicator).
  const owner = () => store.accounts.find((a) => a.id === props.conversation.accountId);
  const showAccountIndicator = () => store.accounts.length > 1;
  const ownerInitial = () => (owner()?.displayName?.trim()?.[0] ?? "?").toUpperCase();
  // Reactive accessor (not a captured value): re-reads on every
  // messagesByConversation change, so the unread badge clears the instant a
  // conversation is opened (markAllMessagesRead), not only after the row
  // remounts on navigating away and back.
  const unread = () => unreadCount(props.conversation);
  // Format a group system event (e.g. "You made Alice an admin") for the
  // preview when the last message is one; otherwise show the raw last message.
  const preview = () => {
    const c = props.conversation;
    if (c.lastMessageKind > 0) {
      return groupEventText(
        c.lastMessageMetadata,
        c.lastMessage ?? "",
        c.accountId,
        (d) => displayName(d, c.accountId)
      );
    }
    return c.lastMessage ?? "";
  };
  const did =
    props.conversation.recipientDid ??
    props.conversation.groupId ??
    props.conversation.id;

  return (
    <div
      class={`conversation-row${props.selected ? " selected" : ""}`}
      onClick={() => props.onSelect(props.conversation.id)}
    >
      {/* DMs resolve bot status reactively; groups are never bots (and groupId
          is not a DID to look up), so force isBot=false for them. */}
      <ContactAvatar
        name={props.conversation.title}
        did={did}
        accountId={props.conversation.accountId}
        isBot={props.conversation.isGroup ? false : undefined}
      />

      <div class="conv-info">
        <div class="conv-title">{props.conversation.title}</div>
        {preview() && <div class="conv-preview">{preview()}</div>}
      </div>
      <div class="conv-meta">
        {props.conversation.lastMessageDate && (
          <span class="conv-timestamp">
            {formatRelative(props.conversation.lastMessageDate)}
          </span>
        )}
        <div class="conv-badges">
          <Show when={showAccountIndicator()}>
            <span class="conv-account-badge" title={owner()?.displayName ?? ""}>
              {ownerInitial()}
            </span>
          </Show>
          {unread() > 0 && <span class="unread-badge">{unread()}</span>}
        </div>
      </div>
    </div>
  );
}
