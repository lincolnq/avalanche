import {
  createEffect,
  createMemo,
  createSignal,
  onMount,
  For,
  Show,
  Switch,
  Match,
} from "solid-js";
import { useApp } from "../../state/AppContext";
import type { Conversation, Message } from "../../models";
import { initials } from "../../lib/format";
import MessageBubble from "../../components/MessageBubble";
import ComposeMessageView from "../../components/ComposeMessageView";
import EditHistorySheet from "../../components/EditHistorySheet";
import DisappearingMessagesPicker from "../../components/DisappearingMessagesPicker";
import "./ConversationView.css";

interface Props {
  conversation: Conversation;
}

export default function ConversationView(props: Props) {
  const app = useApp();
  const {
    store,
    loadMessagesFromStore,
    markAllMessagesRead,
    displayName,
    loadReactions,
    acceptRequest,
    deleteRequest,
    reportAndBlock,
    unblockContact,
    getConversationTimer,
    setConversationTimer,
  } = app;
  let messagesEnd: HTMLDivElement | undefined;

  const [editingMessage, setEditingMessage] = createSignal<Message | null>(null);
  const [historyMessage, setHistoryMessage] = createSignal<Message | null>(null);
  const [timerSecs, setTimerSecs] = createSignal(0);

  const isDm = () => !props.conversation.isGroup && !!props.conversation.recipientDid;

  // Re-runs whenever conversation changes, not just on first mount. Loads both
  // the message timeline and its reaction clusters, and the DM timer.
  createEffect(() => {
    loadMessagesFromStore(props.conversation.id, props.conversation.accountId);
    loadReactions(props.conversation.id);
    // Cancel any in-progress edit/history when switching conversations.
    setEditingMessage(null);
    setHistoryMessage(null);
    // Load the disappearing-messages timer for DMs (groups manage their own
    // timer in the group detail view).
    if (isDm()) {
      void getConversationTimer(props.conversation.id).then((s) => setTimerSecs(s ?? 0));
    } else {
      setTimerSecs(0);
    }
  });

  // Mark all messages read when messages arrive (handles both initial async
  // load and new incoming messages).  Tracking messages().length ensures this
  // re-runs after the async fetch resolves.  Don't ack reads on a conversation
  // that is still a pending request — opening it to evaluate isn't acceptance.
  createEffect(() => {
    const msgs = messages();
    msgs.length; // track — re-run when messages actually arrive
    if (!props.conversation.isRequest) {
      markAllMessagesRead(props.conversation.id, props.conversation.accountId);
    }
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

  function changeTimer(secs: number) {
    setTimerSecs(secs);
    const recipientDid = props.conversation.recipientDid;
    if (recipientDid) void setConversationTimer(recipientDid, secs === 0 ? null : secs);
  }

  return (
    <div class="conv-view">
      <div class="conv-header">
        <div class="conv-header-main">
          <div class="conv-header-avatar">{initials(props.conversation.title)}</div>
          {props.conversation.title}
        </div>
        <Show when={isDm()}>
          <div class="conv-header-timer">
            <span class="conv-header-timer-label">Disappearing</span>
            <DisappearingMessagesPicker seconds={timerSecs()} onChange={changeTimer} />
          </div>
        </Show>
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

      <Switch
        fallback={
          <ComposeMessageView
            conversation={props.conversation}
            editingMessage={editingMessage()}
            onCancelEdit={() => setEditingMessage(null)}
          />
        }
      >
        <Match when={props.conversation.isRequest}>
          <div class="request-banner">
            <p class="request-text">
              Let {props.conversation.title} message you and share your name with them?
            </p>
            <div class="request-actions">
              <button
                class="request-block"
                onClick={() =>
                  void reportAndBlock(props.conversation, "Blocked from message request")
                }
              >
                Block
              </button>
              <button
                class="request-delete"
                onClick={() => void deleteRequest(props.conversation)}
              >
                Delete
              </button>
              <button
                class="request-accept"
                onClick={() => void acceptRequest(props.conversation)}
              >
                Accept
              </button>
            </div>
          </div>
        </Match>
        <Match when={props.conversation.isBlocked}>
          <div class="blocked-bar">
            <span>You blocked this contact.</span>
            <Show when={props.conversation.recipientDid}>
              <button
                class="blocked-unblock-btn"
                onClick={() => void unblockContact(props.conversation.recipientDid!)}
              >
                Unblock
              </button>
            </Show>
          </div>
        </Match>
      </Switch>

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
