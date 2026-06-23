import { createSignal, createEffect, createMemo, onMount, For } from "solid-js";
import { useApp } from "../../state/AppContext";
import type { Conversation } from "../../models";
import { DeliveryStatus } from "../../models/Message";
import { initials } from "../../lib/format";
import "./ConversationView.css";

function formatTime(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

function statusLabel(status: DeliveryStatus): string {
  if (status === DeliveryStatus.sending) return "Sending…";
  if (status === DeliveryStatus.failed) return "Failed";
  if (status === DeliveryStatus.read) return "Read";
  return "";
}

interface Props {
  conversation: Conversation;
}

export default function ConversationView(props: Props) {
  const { store, loadMessagesFromStore, sendMessage, sendGroupMessage } = useApp();
  const [draft, setDraft] = createSignal("");
  const [sending, setSending] = createSignal(false);
  let messagesEnd: HTMLDivElement | undefined;
  let inputRef: HTMLTextAreaElement | undefined;

  // Re-runs whenever conversation changes, not just on first mount.
  createEffect(() => {
    loadMessagesFromStore(props.conversation.id, props.conversation.accountId);
  });

  onMount(() => inputRef?.focus());

  // createMemo ensures the For list re-renders when the async store write lands.
  const messages = createMemo(() => store.messagesByConversation[props.conversation.id] ?? []);
  const myDid = () => props.conversation.accountId;

  function scrollToBottom() {
    messagesEnd?.scrollIntoView({ behavior: "smooth" });
  }

  async function handleSend() {
    const text = draft().trim();
    if (!text || sending()) return;
    setDraft("");
    setSending(true);
    try {
      if (props.conversation.isGroup) {
        await sendGroupMessage(props.conversation, text);
      } else if (props.conversation.recipientDid) {
        await sendMessage(
          props.conversation.id,
          text,
          props.conversation.recipientDid,
          props.conversation.accountId
        );
      }
    } catch {
      // optimistic update already shows failed state
    } finally {
      setSending(false);
      scrollToBottom();
    }
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void handleSend();
    }
  }

  return (
    <div class="conv-view">
        <div class="conv-header">
          <div class="conv-header-avatar">{initials(props.conversation.title)}</div>
          {props.conversation.title}
        </div>
        <div class="messages-list">
          <For each={messages()}>
            {(msg) => {
              const mine = msg.senderAccountId === myDid();
              return (
                <div class={`message-row ${mine ? "mine" : "theirs"}`}>
                  <div class="bubble">{msg.body}</div>
                  <div class="message-meta">
                    {formatTime(msg.sentAtMs)}
                    {mine && msg.deliveryStatus !== DeliveryStatus.sent && (
                      <span> · {statusLabel(msg.deliveryStatus)}</span>
                    )}
                  </div>
                </div>
              );
            }}
          </For>
          <div ref={messagesEnd} />
        </div>
        <div class="compose-row">
          <textarea
            ref={inputRef}
            class="compose-input"
            placeholder="Message"
            rows={1}
            value={draft()}
            onInput={(e) => setDraft(e.currentTarget.value)}
            onKeyDown={handleKeyDown}
          />
          <button
            class="send-btn"
            disabled={!draft().trim() || sending()}
            onClick={handleSend}
          >
            ↑
          </button>
        </div>
      </div>
  );
}
