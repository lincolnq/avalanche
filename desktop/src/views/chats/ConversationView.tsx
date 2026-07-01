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
import { FiUsers } from "solid-icons/fi";
import { useApp } from "../../state/AppContext";
import type { Conversation, Message } from "../../models";
import { initials } from "../../lib/format";
import { groupEventText } from "../../lib/groupEvents";
import MessageBubble from "../../components/MessageBubble";
import ComposeMessageView from "../../components/ComposeMessageView";
import EditHistorySheet from "../../components/EditHistorySheet";
import GroupDetailView from "../../components/GroupDetailView";
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
  const [showGroupDetail, setShowGroupDetail] = createSignal(false);
  const [timerSecs, setTimerSecs] = createSignal(0);
  // Group membership, read from app-core's persistent store (survives refresh,
  // unlike the in-memory hasLeft flag). Default true to avoid flashing the
  // read-only notice for groups you're in while the check resolves. The
  // generation counter discards a stale in-flight resolve when the user
  // switches to another conversation before it lands.
  const [groupMember, setGroupMember] = createSignal(true);
  let groupMemberGen = 0;

  const isDm = () => !props.conversation.isGroup && !!props.conversation.recipientDid;
  // A group you've left or been removed from: composer is replaced by a notice.
  const isLeftGroup = () =>
    props.conversation.isGroup &&
    (props.conversation.hasLeft === true || !groupMember());

  // Re-runs whenever conversation changes, not just on first mount. Loads both
  // the message timeline and its reaction clusters, and the DM timer.
  createEffect(() => {
    loadMessagesFromStore(props.conversation.id, props.conversation.accountId);
    loadReactions(props.conversation.id);
    // Cancel any in-progress edit/history/detail when switching conversations.
    setEditingMessage(null);
    setHistoryMessage(null);
    setShowGroupDetail(false);
    // Load the disappearing-messages timer for DMs (groups manage their own
    // timer in the group detail view). The DM timer is keyed by peer DID in
    // app-core's store, so read it with recipientDid — not the conversation id.
    const recipientDid = props.conversation.recipientDid;
    if (isDm() && recipientDid) {
      void getConversationTimer(props.conversation.accountId, recipientDid).then((s) =>
        setTimerSecs(s ?? 0)
      );
    } else {
      setTimerSecs(0);
    }
    // Resolve group membership from the persistent store so the read-only state
    // is correct after a refresh (when the in-memory hasLeft flag is gone).
    const groupId = props.conversation.groupId;
    const gen = ++groupMemberGen;
    setGroupMember(true);
    if (props.conversation.isGroup && groupId) {
      void app
        .serviceFor(props.conversation.accountId)
        .isGroupMember(groupId)
        .then((m) => {
          // Ignore a resolve that lost the race to a later conversation switch.
          if (gen === groupMemberGen) setGroupMember(m);
        })
        .catch(() => {});
    }
  });

  // Re-check group membership when the open group's metadata changes (T74):
  // being removed by another admin while viewing the group must flip the
  // composer to the read-only notice without waiting for a conversation switch.
  createEffect(() => {
    const change = app.groupMetaChange(); // track
    const groupId = props.conversation.groupId;
    if (!props.conversation.isGroup || !groupId || change.groupId !== groupId) return;
    const gen = ++groupMemberGen;
    void app
      .serviceFor(props.conversation.accountId)
      .isGroupMember(groupId)
      .then((m) => {
        if (gen === groupMemberGen) setGroupMember(m);
      })
      .catch(() => {});
  });

  // Mark all messages read when messages arrive (handles both initial async
  // load and new incoming messages).  Tracking messages().length ensures this
  // re-runs after the async fetch resolves.  This always clears the local
  // unread state (so the badge clears); the read *receipt* to the sender is
  // separately suppressed for request/blocked conversations inside
  // markAllMessagesRead.
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

  function changeTimer(secs: number) {
    const recipientDid = props.conversation.recipientDid;
    if (!recipientDid) return;
    setTimerSecs(secs); // optimistic
    const accountId = props.conversation.accountId;
    void setConversationTimer(accountId, recipientDid, secs === 0 ? null : secs).finally(() => {
      // Re-read the authoritative stored value so the picker reverts if the
      // write failed (mirrors the group setExpiry reload-after-write path).
      void getConversationTimer(accountId, recipientDid).then((s) => setTimerSecs(s ?? 0));
    });
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
        <Show when={props.conversation.isGroup}>
          <button
            class="conv-header-info"
            onClick={() => setShowGroupDetail(true)}
            aria-label="Group info"
            title="Group info"
          >
            <FiUsers size={18} />
          </button>
        </Show>
      </div>
      <div class="messages-list scrollbar-thin">
        <Show
          when={messages().length > 0}
          fallback={<div class="empty-conv">No messages yet.</div>}
        >
          <For each={messages()}>
            {(msg) => (
              <Show
                when={msg.kind > 0}
                fallback={
                  <MessageBubble
                    conversation={props.conversation}
                    message={msg}
                    mine={msg.senderAccountId === props.conversation.accountId}
                    isGroup={props.conversation.isGroup}
                    senderName={displayName(msg.senderAccountId, props.conversation.accountId)}
                    onEdit={(m) => setEditingMessage(m)}
                    onShowHistory={(m) => setHistoryMessage(m)}
                  />
                }
              >
                {/* Group membership/metadata event (docs/03 §3.6) — a centered
                    grey system line, e.g. "You made Alice an admin". */}
                <div class="system-event">
                  {groupEventText(
                    msg.metadata,
                    msg.body,
                    props.conversation.accountId,
                    (d) => displayName(d, props.conversation.accountId)
                  )}
                </div>
              </Show>
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
                onClick={() =>
                  void unblockContact(
                    props.conversation.accountId,
                    props.conversation.recipientDid!
                  )
                }
              >
                Unblock
              </button>
            </Show>
          </div>
        </Match>
        <Match when={isLeftGroup()}>
          <div class="left-group-bar">You are no longer a member of this group.</div>
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
      <Show when={showGroupDetail()}>
        <GroupDetailView
          conversation={props.conversation}
          onClose={() => setShowGroupDetail(false)}
        />
      </Show>
    </div>
  );
}
