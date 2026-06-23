import { createSignal, For, Show } from "solid-js";
import { useApp } from "../../state/AppContext";
import type { Conversation } from "../../models";
import { initials } from "../../lib/format";
import ConversationView from "./ConversationView";
import "./ChatsView.css";


export default function ChatsView() {
  const { store } = useApp();
  const [selectedId, setSelectedId] = createSignal<string | null>(null);

  const selected = () =>
    store.conversations.find((c) => c.id === selectedId()) ?? null;

  return (
    <div class="chats-split">
        <div class="chats-list-panel">
          <div class="chats-header">Chats</div>
          <div class="conversation-list">
            <For
              each={store.conversations}
              fallback={
                <div class="empty-state">
                  No conversations yet. Join a server to get started.
                </div>
              }
            >
              {(conv) => (
                <div
                  class={`conversation-row${selectedId() === conv.id ? " selected" : ""}`}
                  onClick={() => setSelectedId(conv.id)}
                >
                  <div class="conv-avatar">{initials(conv.title)}</div>
                  <div class="conv-info">
                    <div class="conv-title">{conv.title}</div>
                    {conv.lastMessage && (
                      <div class="conv-preview">{conv.lastMessage}</div>
                    )}
                  </div>
                </div>
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
      </div>
  );
}
