import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
  Switch,
  Match,
} from "solid-js";
import { useApp } from "../../state/AppContext";
import type { Conversation, Message } from "../../models";
import { groupEventText } from "../../lib/groupEvents";
import MessageBubble from "../../components/MessageBubble";
import ContactAvatar from "../../components/ContactAvatar";
import ComposeMessageView from "../../components/ComposeMessageView";
import EditHistorySheet from "../../components/EditHistorySheet";
import ImageViewerModal from "../../components/ImageViewerModal";
import GroupDetailView from "../../components/GroupDetailView";
import ConversationInfoView from "../../components/ConversationInfoView";
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
  } = app;

  const [editingMessage, setEditingMessage] = createSignal<Message | null>(null);
  const [historyMessage, setHistoryMessage] = createSignal<Message | null>(null);
  // The attachment id of the image tapped to open the fullscreen viewer (docs/35).
  const [imageViewerStartId, setImageViewerStartId] = createSignal<string | null>(null);
  // Conversation controls open by clicking the header name/avatar: the group
  // detail modal for groups, the DM conversation-info modal for DMs.
  const [showGroupDetail, setShowGroupDetail] = createSignal(false);
  const [showConvInfo, setShowConvInfo] = createSignal(false);
  // Group membership, read from app-core's persistent store (survives refresh,
  // unlike the in-memory hasLeft flag). Default true to avoid flashing the
  // read-only notice for groups you're in while the check resolves. The
  // generation counter discards a stale in-flight resolve when the user
  // switches to another conversation before it lands.
  const [groupMember, setGroupMember] = createSignal(true);
  let groupMemberGen = 0;

  // A group you've left or been removed from: composer is replaced by a notice.
  const isLeftGroup = () =>
    props.conversation.isGroup &&
    (props.conversation.hasLeft === true || !groupMember());

  // Re-runs whenever conversation changes, not just on first mount. Loads both
  // the message timeline and its reaction clusters.
  createEffect(() => {
    loadMessagesFromStore(props.conversation.id, props.conversation.accountId);
    loadReactions(props.conversation.id);
    // Cancel any in-progress edit/history/detail when switching conversations.
    setEditingMessage(null);
    setHistoryMessage(null);
    setShowGroupDetail(false);
    setShowConvInfo(false);
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

  // createMemo ensures the For list re-renders when the async store write lands.
  const messages = createMemo(() => store.messagesByConversation[props.conversation.id] ?? []);

  // Runs of consecutive messages from one sender (mirrors iOS ConversationView
  // senderName(for:at:) / isLastInRun(at:)): the sender name shows only on the
  // first message of a run and the timestamp/delivery only on the last. A
  // system event breaks a run.
  const sameRun = (a: Message | undefined, b: Message | undefined) =>
    !!a && !!b && a.kind === 0 && b.kind === 0 && a.senderAccountId === b.senderAccountId;
  const isFirstInRun = (i: number) => !sameRun(messages()[i - 1], messages()[i]);
  const isLastInRun = (i: number) => !sameRun(messages()[i], messages()[i + 1]);

  // Every image attachment in the conversation, in timeline order (message
  // order, then attachment order) — the set the fullscreen viewer pages through.
  const conversationImages = createMemo(() =>
    messages()
      .flatMap((m) => m.attachments ?? [])
      .filter((a) => a.contentType.startsWith("image/")),
  );

  // Bottom-pinned timeline. Opening a conversation jumps to the newest
  // message; while the view is at (or near) the bottom it stays there as
  // content grows — new messages, but also rows that grow after first paint
  // (sender names resolving, images and previews loading). Scrolling up
  // unpins, so reading history isn't yanked away.
  let listEl: HTMLDivElement | undefined;
  let contentEl: HTMLDivElement | undefined;
  let pinned = true;
  let scrolledConvId: string | undefined;
  const toBottom = (smooth: boolean) =>
    listEl?.scrollTo({ top: listEl.scrollHeight, behavior: smooth ? "smooth" : "auto" });
  function onScroll() {
    if (!listEl) return;
    pinned = listEl.scrollHeight - listEl.clientHeight - listEl.scrollTop < 48;
  }
  createEffect(() => {
    const convId = props.conversation.id;
    const count = messages().length; // track — re-run when messages load/arrive
    if (count === 0) return; // nothing to anchor to yet
    const switched = scrolledConvId !== convId;
    scrolledConvId = convId;
    if (switched) pinned = true;
    // A conversation switch jumps; a new message in an open one animates.
    if (pinned) queueMicrotask(() => toBottom(!switched));
  });
  onMount(() => {
    const ro = new ResizeObserver(() => {
      if (pinned) toBottom(false);
    });
    if (contentEl) ro.observe(contentEl);
    onCleanup(() => ro.disconnect());
  });

  return (
    <div class="conv-view">
      {/* The header row is the window drag strip. The name/avatar is a button
          (opens conversation info) so it keeps its click; dragging works from the
          empty header space around it. */}
      <div class="conv-header" data-tauri-drag-region>
        {/* All conversation controls open from the header name/avatar: the
            group detail modal for groups, the DM conversation-info modal
            (disappearing timer, etc.) for DMs. Mirrors iOS's tap-title pattern. */}
        <button
          class="conv-header-main"
          onClick={() => {
            // A group invite request isn't joined: no group detail to show.
            if (props.conversation.isGroup && props.conversation.isRequest) return;
            if (props.conversation.isGroup) setShowGroupDetail(true);
            else setShowConvInfo(true);
          }}
          aria-label="Conversation info"
          title="Conversation info"
        >
          <ContactAvatar
            name={props.conversation.title}
            did={props.conversation.recipientDid ?? props.conversation.groupId ?? props.conversation.id}
            accountId={props.conversation.accountId}
            isBot={props.conversation.isGroup ? false : undefined}
            groupGlyph={props.conversation.isGroup && props.conversation.isRequest}
            groupId={props.conversation.isGroup ? props.conversation.groupId : undefined}
            size="sm"
          />
          {props.conversation.title}
        </button>
      </div>
      <div class="messages-list scrollbar-thin" ref={listEl} onScroll={onScroll}>
        <div class="messages-content" ref={contentEl}>
        <Show
          when={messages().length > 0}
          fallback={<div class="empty-conv">No messages yet.</div>}
        >
          <For each={messages()}>
            {(msg, i) => (
              <Show
                when={msg.kind > 0}
                fallback={
                  <MessageBubble
                    conversation={props.conversation}
                    message={msg}
                    mine={msg.senderAccountId === props.conversation.accountId}
                    isGroup={props.conversation.isGroup}
                    senderName={
                      isFirstInRun(i())
                        ? displayName(msg.senderAccountId, props.conversation.accountId)
                        : undefined
                    }
                    firstInRun={isFirstInRun(i())}
                    lastInRun={isLastInRun(i())}
                    onEdit={(m) => setEditingMessage(m)}
                    onShowHistory={(m) => setHistoryMessage(m)}
                    onImageClick={(a) => setImageViewerStartId(a.id)}
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
        </div>
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
              <Show
                when={props.conversation.isGroup}
                fallback={<>Let {props.conversation.title} message you and share your name with them?</>}
              >
                {props.conversation.inviterDid
                  ? displayName(props.conversation.inviterDid, props.conversation.accountId)
                  : "Someone"}{" "}
                invited you to this group. Join to see its messages and let its members see your name?
              </Show>
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
                {props.conversation.isGroup ? "Join" : "Accept"}
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
      <Show when={showConvInfo()}>
        <ConversationInfoView
          conversation={props.conversation}
          onClose={() => setShowConvInfo(false)}
        />
      </Show>
      <Show when={imageViewerStartId()}>
        {(startId) => (
          <ImageViewerModal
            images={conversationImages()}
            startId={startId()}
            accountId={props.conversation.accountId}
            onClose={() => setImageViewerStartId(null)}
          />
        )}
      </Show>
    </div>
  );
}
