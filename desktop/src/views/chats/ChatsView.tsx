import { createSignal, For, Show } from "solid-js";
import { useApp } from "../../state/AppContext";
import type { Conversation } from "../../models";
import ConversationView from "./ConversationView";

const styles = `
  .chats-split {
    display: flex;
    height: 100%;
  }
  .chats-list-panel {
    width: 280px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    border-right: 1px solid rgba(42,22,32,0.12);
    background: #FFF1E9;
  }
  .chats-header {
    padding: 16px 20px 12px;
    font-size: 18px;
    font-weight: 600;
    color: #1F1815;
    border-bottom: 1px solid rgba(42,22,32,0.12);
    flex-shrink: 0;
  }
  .conversation-list {
    flex: 1;
    overflow-y: auto;
  }
  .conversation-row {
    display: flex;
    align-items: center;
    padding: 12px 16px;
    border-bottom: 1px solid rgba(42,22,32,0.08);
    cursor: pointer;
    transition: background 0.1s;
  }
  .conversation-row:hover {
    background: rgba(107,62,80,0.06);
  }
  .conversation-row.selected {
    background: rgba(107,62,80,0.12);
  }
  .conv-avatar {
    width: 40px;
    height: 40px;
    border-radius: 50%;
    background: rgba(107,62,80,0.2);
    color: #6B3E50;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 16px;
    font-weight: 500;
    flex-shrink: 0;
    margin-right: 12px;
  }
  .conv-info {
    flex: 1;
    min-width: 0;
  }
  .conv-title {
    font-size: 14px;
    font-weight: 600;
    color: #1F1815;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .conv-preview {
    font-size: 13px;
    color: #6E6258;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-top: 2px;
  }
  .empty-state {
    padding: 40px 16px;
    text-align: center;
    color: #6E6258;
    font-size: 14px;
  }
  .detail-panel {
    flex: 1;
    overflow: hidden;
  }
  .no-selection {
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #6E6258;
    font-size: 14px;
    background: #FFF1E9;
  }
`;

function initials(title: string): string {
  return title.split(/\s+/).slice(0, 2).map((w) => w[0]?.toUpperCase() ?? "").join("");
}

export default function ChatsView() {
  const { store } = useApp();
  const [selectedId, setSelectedId] = createSignal<string | null>(null);

  const selected = () =>
    store.conversations.find((c) => c.id === selectedId()) ?? null;

  return (
    <>
      <style>{styles}</style>
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
    </>
  );
}
