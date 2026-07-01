import { createSignal, createMemo, For, Show } from "solid-js";
import { FiEdit } from "solid-icons/fi";
import { useApp } from "../../state/AppContext";
import ConversationRow from "../../components/ConversationRow";
import RecoveryKeyBanner from "../../components/RecoveryKeyBanner";
import OfflineBanner from "../../components/OfflineBanner";
import NewConversationView from "../../components/NewConversationView";
import ConversationView from "./ConversationView";
import "./ChatsView.css";

export default function ChatsView() {
  const { store, loadMessagesFromStore, unreadCount, selectedConversationId, selectConversation } =
    useApp();
  const [showNew, setShowNew] = createSignal(false);

  const selected = () =>
    store.conversations.find((c) => c.id === selectedConversationId()) ?? null;

  const totalUnread = createMemo(() =>
    store.conversations.reduce((sum, c) => sum + unreadCount(c), 0)
  );

  return (
    <div class="chats-split">
      <div class="chats-list-panel">
        <div class="chats-header">
          <span class="chats-header-title">
            Chats
            {totalUnread() > 0 && (
              <span class="chats-unread-badge">{totalUnread()}</span>
            )}
          </span>
          <button
            class="chats-new-btn"
            onClick={() => setShowNew(true)}
            aria-label="New message"
            title="New message"
          >
            <FiEdit size={18} />
          </button>
        </div>
        <RecoveryKeyBanner />
        <OfflineBanner />
        <div class="conversation-list scrollbar-thin">
          <For
            each={store.conversations}
            fallback={
              <div class="empty-state">
                No conversations yet. Join a server to get started.
              </div>
            }
          >
            {(conv) => (
              <ConversationRow
                conversation={conv}
                selected={selectedConversationId() === conv.id}
                onSelect={(id) => {
                  selectConversation(id);
                  loadMessagesFromStore(id, conv.accountId);
                }}
              />
            )}
          </For>
        </div>
      </div>
      <div class="detail-panel">
        <Show
          when={selected()}
          fallback={<div class="no-selection">Select a conversation</div>}
        >
          {(conv) => <ConversationView conversation={conv()} />}
        </Show>
      </div>
      <Show when={showNew()}>
        <NewConversationView onClose={() => setShowNew(false)} />
      </Show>
    </div>
  );
}
