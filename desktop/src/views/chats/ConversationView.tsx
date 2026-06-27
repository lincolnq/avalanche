import { createEffect, createMemo, createSignal, onMount, For, Show } from "solid-js";
import { useApp } from "../../state/AppContext";
import type { Conversation, Message } from "../../models";
import { initials } from "../../lib/format";
import MessageBubble from "../../components/MessageBubble";
import ComposeMessageView from "../../components/ComposeMessageView";
import EditHistorySheet from "../../components/EditHistorySheet";
import "./ConversationView.css";

interface Props {
  conversation: Conversation;
}

export default function ConversationView(props: Props) {
  const { store, loadMessagesFromStore, markAllMessagesRead, displayName, loadReactions } =
    useApp();
  let messagesEnd: HTMLDivElement | undefined;

  const [editingMessage, setEditingMessage] = createSignal<Message | null>(null);
  const [historyMessage, setHistoryMessage] = createSignal<Message | null>(null);

  // Re-runs whenever conversation changes, not just on first mount. Loads both
  // the message timeline and its reaction clusters.
  createEffect(() => {
    loadMessagesFromStore(props.conversation.id, props.conversation.accountId);
    loadReactions(props.conversation.id);
    // Cancel any in-progress edit when switching conversations.
    setEditingMessage(null);
    setHistoryMessage(null);
  });

  // Mark all messages read when messages arrive (handles both initial async
  // load and new incoming messages).  Tracking messages().length ensures this
  // re-runs after the async fetch resolves — the conversation-id-only effect
  // would fire before messages arrived from disk.
  createEffect(() => {
    const msgs = messages();
    msgs.length; // track — re-run when messages actually arrive
    markAllMessagesRead(props.conversation.id, props.conversation.accountId);
  });

  onMount(() => {
    messagesEnd?.scrollIntoView();
  });

  // createMemo ensures the For list re-renders when the async store write lands.
  const messages = createMemo(() => store.messagesByConversation[props.conversation.id] ?? []);

  // Auto-scroll when message count changes (new messages arrive or are sent).
  createEffect(() => {
    messages().length; // track
    messagesEnd?.scrollIntoView({ behavior: "smooth" });
  });

  return (
    <div class="conv-view">
      <div class="conv-header">
        <div class="conv-header-avatar">{initials(props.conversation.title)}</div>
        {props.conversation.title}
      </div>
      <div class="messages-list scrollbar-thin">
        <Show
          when={messages().length > 0}
          fallback={<div class="empty-conv">No messages yet.</div>}
        >
          <For each={messages()}>
            {(msg) => (
              <MessageBubble
                conversation={props.conversation}
                message={msg}
                mine={msg.senderAccountId === props.conversation.accountId}
                isGroup={props.conversation.isGroup}
                senderName={displayName(msg.senderAccountId, props.conversation.accountId)}
                onEdit={(m) => setEditingMessage(m)}
                onShowHistory={(m) => setHistoryMessage(m)}
              />
            )}
          </For>
        </Show>
        <div ref={messagesEnd} />
      </div>
      <ComposeMessageView
        conversation={props.conversation}
        editingMessage={editingMessage()}
        onCancelEdit={() => setEditingMessage(null)}
      />
      <Show when={historyMessage()}>
        {(m) => (
          <EditHistorySheet
            conversation={props.conversation}
            message={m()}
            onClose={() => setHistoryMessage(null)}
          />
        )}
      </Show>
    </div>
  );
}
