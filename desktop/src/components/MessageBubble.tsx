import { Switch, Match, For, Show, createSignal } from "solid-js";
import {
  TbOutlineClock,
  TbOutlineCheck,
  TbOutlineChecks,
  TbOutlineAlertTriangle,
} from "solid-icons/tb";
import { FiMoreHorizontal } from "solid-icons/fi";
import { useApp } from "../state/AppContext";
import type { Conversation, Message } from "../models";
import { DeliveryStatus } from "../models/Message";
import { formatTime } from "../lib/format";
import "./MessageBubble.css";

const DELIVERY_ICON_SIZE = 14;
const QUICK_EMOJI = ["👍", "❤️", "😂", "😮", "😢", "🙏"];

interface Props {
  conversation: Conversation;
  message: Message;
  mine: boolean;
  isGroup: boolean;
  senderName?: string;
  onEdit: (message: Message) => void;
  onShowHistory: (message: Message) => void;
}

export default function MessageBubble(props: Props) {
  const app = useApp();
  const [menuOpen, setMenuOpen] = createSignal(false);
  const deleted = () => props.message.isDeleted;
  const myDid = () => props.conversation.accountId;
  const canEdit = () => props.mine && !deleted();

  // Reaction clusters grouped by emoji, preserving first-appearance order.
  const clusters = () => {
    const list = app.reactionsFor(props.conversation, props.message);
    const order: string[] = [];
    const byEmoji = new Map<string, { count: number; mine: boolean }>();
    for (const r of list) {
      const c = byEmoji.get(r.emoji);
      if (c) {
        c.count++;
        if (r.reactorDid === myDid()) c.mine = true;
      } else {
        order.push(r.emoji);
        byEmoji.set(r.emoji, { count: 1, mine: r.reactorDid === myDid() });
      }
    }
    return order.map((e) => {
      const c = byEmoji.get(e)!;
      return { emoji: e, count: c.count, mine: c.mine };
    });
  };

  function openMenu(e: MouseEvent) {
    e.preventDefault();
    if (!deleted()) setMenuOpen(true);
  }
  const closeMenu = () => setMenuOpen(false);

  function react(emoji: string) {
    app.toggleReaction(props.conversation, props.message, emoji);
    closeMenu();
  }
  function copy() {
    void navigator.clipboard.writeText(props.message.body);
    closeMenu();
  }
  function del(forEveryone: boolean) {
    app.deleteMessage(props.conversation, props.message, forEveryone);
    closeMenu();
  }

  return (
    <div class={`message-row ${props.mine ? "mine" : "theirs"}`}>
      {props.isGroup && !props.mine && props.senderName && (
        <span class="sender-name">{props.senderName}</span>
      )}
      <div class="bubble-wrap">
        {deleted() ? (
          <div class="deleted-tombstone">This message was deleted</div>
        ) : (
          <>
            <div class="bubble" onContextMenu={openMenu}>
              {props.message.body}
            </div>
            <button
              class="bubble-menu-btn"
              aria-label="Message actions"
              onClick={() => setMenuOpen(true)}
            >
              <FiMoreHorizontal size={14} />
            </button>
          </>
        )}
        <Show when={menuOpen()}>
          <div class="context-menu-backdrop" onClick={closeMenu} />
          <div class="context-menu" classList={{ mine: props.mine }}>
            <div class="ctx-emoji-row">
              <For each={QUICK_EMOJI}>
                {(e) => (
                  <button class="ctx-emoji" onClick={() => react(e)}>
                    {e}
                  </button>
                )}
              </For>
            </div>
            <Show when={canEdit()}>
              <button
                class="ctx-item"
                onClick={() => {
                  props.onEdit(props.message);
                  closeMenu();
                }}
              >
                Edit
              </button>
            </Show>
            <Show when={props.message.editCount > 0}>
              <button
                class="ctx-item"
                onClick={() => {
                  props.onShowHistory(props.message);
                  closeMenu();
                }}
              >
                Edit History
              </button>
            </Show>
            <button class="ctx-item" onClick={copy}>
              Copy
            </button>
            <Show when={props.mine}>
              <button class="ctx-item danger" onClick={() => del(true)}>
                Delete for Everyone
              </button>
            </Show>
            <button class="ctx-item danger" onClick={() => del(false)}>
              Delete for Me
            </button>
          </div>
        </Show>
      </div>
      <Show when={clusters().length > 0}>
        <div class="reaction-row">
          <For each={clusters()}>
            {(c) => (
              <button
                class="reaction-pill"
                classList={{ mine: c.mine }}
                onClick={() =>
                  app.toggleReaction(props.conversation, props.message, c.emoji)
                }
              >
                <span>{c.emoji}</span>
                {c.count > 1 && <span class="reaction-count">{c.count}</span>}
              </button>
            )}
          </For>
        </div>
      </Show>
      {!deleted() && (
        <div class="message-meta">
          <span class="timestamp">
            {formatTime(props.message.sentAtMs)}
            {props.message.editCount > 0 && " (edited)"}
          </span>
          {props.mine && (
            <DeliveryIndicator
              status={props.message.deliveryStatus}
              onRetry={() => void app.retryMessage(props.conversation, props.message)}
            />
          )}
        </div>
      )}
    </div>
  );
}

function DeliveryIndicator(props: { status: DeliveryStatus; onRetry: () => void }) {
  return (
    <Switch>
      <Match when={props.status === DeliveryStatus.sending}>
        <span class="delivery sending"><TbOutlineClock size={DELIVERY_ICON_SIZE} /></span>
      </Match>
      <Match when={props.status === DeliveryStatus.sent}>
        <span class="delivery"><TbOutlineCheck size={DELIVERY_ICON_SIZE} /></span>
      </Match>
      <Match when={props.status === DeliveryStatus.delivered}>
        <span class="delivery"><TbOutlineChecks size={DELIVERY_ICON_SIZE} /></span>
      </Match>
      <Match when={props.status === DeliveryStatus.read}>
        <span class="delivery read"><TbOutlineChecks size={DELIVERY_ICON_SIZE} /></span>
      </Match>
      <Match when={props.status === DeliveryStatus.failed}>
        <button class="delivery failed" onClick={props.onRetry}>
          <TbOutlineAlertTriangle size={DELIVERY_ICON_SIZE} />
          <span class="retry-hint">Tap to retry</span>
        </button>
      </Match>
    </Switch>
  );
}
