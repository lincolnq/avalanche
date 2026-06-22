import { createSignal, createEffect, createMemo, onMount, For } from "solid-js";
import { useApp } from "../../state/AppContext";
import type { Conversation } from "../../models";
import { DeliveryStatus } from "../../models/Message";

const styles = `
  .conv-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: #FFF1E9;
  }
  .conv-header {
    padding: 14px 20px;
    font-size: 16px;
    font-weight: 600;
    color: #1F1815;
    border-bottom: 1px solid rgba(42,22,32,0.12);
    display: flex;
    align-items: center;
    gap: 10px;
    flex-shrink: 0;
  }
  .conv-header-avatar {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: rgba(107,62,80,0.2);
    color: #6B3E50;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 13px;
    font-weight: 500;
    flex-shrink: 0;
  }
  .messages-list {
    flex: 1;
    overflow-y: auto;
    padding: 16px 20px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .message-row {
    display: flex;
    flex-direction: column;
    max-width: 72%;
  }
  .message-row.mine {
    align-self: flex-end;
    align-items: flex-end;
  }
  .message-row.theirs {
    align-self: flex-start;
    align-items: flex-start;
  }
  .bubble {
    padding: 9px 13px;
    border-radius: 16px;
    font-size: 14px;
    line-height: 1.45;
    word-break: break-word;
  }
  .mine .bubble {
    background: #2A1620;
    color: #FFF1E9;
    border-bottom-right-radius: 4px;
  }
  .theirs .bubble {
    background: rgba(107,62,80,0.12);
    color: #1F1815;
    border-bottom-left-radius: 4px;
  }
  .message-meta {
    font-size: 11px;
    color: #6E6258;
    margin-top: 3px;
    padding: 0 2px;
  }
  .compose-row {
    display: flex;
    align-items: flex-end;
    gap: 8px;
    padding: 12px 16px;
    border-top: 1px solid rgba(42,22,32,0.12);
    background: #FFF1E9;
    flex-shrink: 0;
  }
  .compose-input {
    flex: 1;
    min-height: 38px;
    max-height: 120px;
    padding: 9px 13px;
    border-radius: 20px;
    border: 1.5px solid rgba(107,62,80,0.25);
    background: #fff;
    font-size: 14px;
    color: #1F1815;
    font-family: inherit;
    resize: none;
    outline: none;
    line-height: 1.4;
  }
  .compose-input:focus {
    border-color: #6B3E50;
  }
  .compose-input::placeholder {
    color: #6E6258;
  }
  .send-btn {
    width: 38px;
    height: 38px;
    border-radius: 50%;
    background: #6B3E50;
    color: #fff;
    border: none;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 16px;
    flex-shrink: 0;
    transition: opacity 0.15s;
  }
  .send-btn:hover { opacity: 0.85; }
  .send-btn:disabled { opacity: 0.4; cursor: default; }
  .empty-conv {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #6E6258;
    font-size: 14px;
  }
`;

function initials(title: string): string {
  return title.split(/\s+/).slice(0, 2).map((w) => w[0]?.toUpperCase() ?? "").join("");
}

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
    <>
      <style>{styles}</style>
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
    </>
  );
}
