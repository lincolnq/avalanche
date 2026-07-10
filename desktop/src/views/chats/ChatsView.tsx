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

  // Sort by recency at render (parity with iOS/Android), not just at store-load:
  // rows appended by findOrCreateDM/GroupConversation and in-place title updates
  // don't re-sort the store, so the render must own the ordering.
  const sortedConversations = createMemo(() =>
    [...store.conversations].sort(
      (a, b) => (b.lastMessageDate ?? 0) - (a.lastMessageDate ?? 0)
    )
  );

  return (
    <div class="chats-split">
      <div class="chats-list-panel">
        {/* The header row is the window drag strip (Signal/WhatsApp): the title
            rides up alongside the macOS traffic lights. The new-message button is
            a child without the attribute, so it stays clickable. */}
        <div class="chats-header" data-tauri-drag-region>
          <span class="chats-header-title" data-tauri-drag-region>
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
            each={sortedConversations()}
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
          fallback={
            <div class="no-selection" data-tauri-drag-region>
              Select a conversation
            </div>
          }
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
