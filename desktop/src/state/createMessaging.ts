import type { SetStoreFunction } from "solid-js/store";
import type { Conversation } from "../models";
import { DeliveryStatus, type Message } from "../models/Message";
import { attachmentPlaceholder } from "../lib/format";
import type {
  ReactionFfi,
  MessageRevisionFfi,
  MessageTarget,
  AttachmentFfi,
  LinkPreviewFfi,
  LinkPreviewMetaFfi,
} from "../services/AvalancheService";
import { messageFromFfi, buildStoredMessage } from "./helpers";
import type { Services } from "./createServices";
import type { AppContextValue, AppStore, SessionGuards } from "./types";

export interface MessagingDeps {
  store: AppStore;
  setStore: SetStoreFunction<AppStore>;
  serviceFor: Services["serviceFor"];
  onboardingService: Services["onboardingService"];
  guards: SessionGuards;
  accountIdForConversation: (conversationId: string) => string | null;
}

// Message timelines: load, optimistic send (DM/group/attachments), read state,
// unread counts, and the Track A message actions (reactions, edit, delete,
// retry) plus the attachment/link-preview service pass-throughs. Pick-typed —
// see the note in createConversations.ts.
export type Messaging = Pick<
  AppContextValue,
  | "sendMessage"
  | "sendGroupMessage"
  | "sendMessageWithAttachments"
  | "uploadAttachment"
  | "downloadAttachment"
  | "fetchLinkPreview"
  | "openExternal"
  | "loadMessagesFromStore"
  | "markAllMessagesRead"
  | "unreadCount"
  | "reactionsFor"
  | "loadReactions"
  | "toggleReaction"
  | "editMessage"
  | "loadMessageRevisions"
  | "deleteMessage"
  | "retryMessage"
> & {
  // Internal API for the other state modules
  reloadMessagesIfLoaded: (cid: string) => void;
  clearReactionsForMessage: (
    conversationId: string,
    targetAuthor: string,
    targetSentAtMs: number
  ) => void;
};

export function createMessaging(deps: MessagingDeps): Messaging {
  const {
    store,
    setStore,
    serviceFor,
    onboardingService,
    guards,
    accountIdForConversation,
  } = deps;

  // Reload a conversation's timeline from the store, fully replacing the
  // in-memory copy (matches iOS `reloadMessagesIfLoaded`). Only acts on an
  // already-loaded conversation, so it never eagerly loads unopened ones. A full
  // replace is correct because the store is the source of truth: a row missing
  // from the reload was deliberately deleted (expired by the disappearing-
  // messages reaper, docs/03 §5, or tombstoned), so it must leave the UI too.
  function reloadMessagesIfLoaded(cid: string) {
    if (!guards.loadedMessages.has(cid)) return;
    const accountId = accountIdForConversation(cid);
    if (accountId === null) return;
    void serviceFor(accountId)
      .loadMessages(cid)
      .then((rows) => {
        setStore("messagesByConversation", cid, rows.map(messageFromFfi));
      })
      .catch((e: unknown) => {
        console.warn("reloadMessagesIfLoaded failed:", cid, e);
      });
  }

  function loadMessagesFromStore(conversationId: string, accountId: string) {
    if (guards.loadedMessages.has(conversationId)) return;
    guards.loadedMessages.add(conversationId);

    void serviceFor(accountId)
      .loadMessages(conversationId)
      .then((rows) => {
        const messages = rows.map(messageFromFfi);
        if (messages.length > 0) {
          setStore("messagesByConversation", conversationId, messages);
        }
      })
      .catch((err) => {
        console.warn("loadMessages failed for", conversationId, err);
        guards.loadedMessages.delete(conversationId);
      });
  }

  async function sendOptimistic(
    conversationId: string,
    text: string,
    senderAccountId: string,
    expireTimerSecs: number,
    transportFn: (sentAtMs: number) => Promise<void>,
    errorMessage: string,
    attachments?: AttachmentFfi[],
    previews?: LinkPreviewFfi[]
  ) {
    const messageId = crypto.randomUUID();
    const sentAtMs = Date.now();

    const optimistic: Message = {
      id: messageId,
      conversationId,
      senderAccountId,
      body: text,
      sentAtMs,
      readAtMs: sentAtMs,
      deliveryStatus: DeliveryStatus.sending,
      editCount: 0,
      isDeleted: false,
      kind: 0,
      // Stamp the sender's local copy with the same disappearing-messages timer
      // the wire envelope carries, so app-core's reaper expires it too. Without
      // this the sender's own messages never disappear (docs/03 §5).
      expireTimerSecs,
      attachments,
      previews,
    };

    setStore("messagesByConversation", conversationId, (prev) => [
      ...(prev ?? []),
      optimistic,
    ]);

    // Update conversation preview. Clear any stale group system-event fields so
    // ConversationRow renders this new message, not a prior "X joined" line.
    // Attachment-only sends (empty body) preview as "Photo"/"Attachment" (iOS parity).
    const previewText =
      text.trim().length > 0
        ? text
        : attachmentPlaceholder(attachments?.[0]?.contentType);
    const convIdx = store.conversations.findIndex((c) => c.id === conversationId);
    if (convIdx >= 0) {
      setStore("conversations", convIdx, "lastMessage", previewText);
      setStore("conversations", convIdx, "lastMessageDate", sentAtMs);
      setStore("conversations", convIdx, "lastMessageKind", 0);
      setStore("conversations", convIdx, "lastMessageMetadata", undefined);
    }

    // Persist every delivery state to the local store so the timeline survives a
    // refresh/restart and a store reload never loses an in-flight or failed
    // send. "sending" is persisted up front (and awaited) so a crash or refresh
    // mid-send is recoverable. Matches iOS AppState.sendMessage.
    const persist = (status: DeliveryStatus) =>
      serviceFor(senderAccountId).saveMessage(
        buildStoredMessage({
          id: messageId,
          conversationId,
          senderDid: senderAccountId,
          body: text,
          sentAtMs,
          readAtMs: sentAtMs,
          deliveryStatus: status,
          expireTimerSecs,
          attachments,
          previews,
        })
      );

    await persist(DeliveryStatus.sending);

    try {
      await transportFn(sentAtMs);
      setStore("messagesByConversation", conversationId, (msgs) =>
        (msgs ?? []).map((m) =>
          m.id === messageId
            ? { ...m, deliveryStatus: DeliveryStatus.sent }
            : m
        )
      );
      // Best-effort re-save — log failures but never crash the send path.
      void persist(DeliveryStatus.sent).catch((err: unknown) => {
        console.warn("saveMessage (sent) failed:", err);
      });
    } catch {
      setStore("messagesByConversation", conversationId, (msgs) =>
        (msgs ?? []).map((m) =>
          m.id === messageId
            ? { ...m, deliveryStatus: DeliveryStatus.failed }
            : m
        )
      );
      void persist(DeliveryStatus.failed).catch((err: unknown) => {
        console.warn("saveMessage (failed) failed:", err);
      });
      throw new Error(errorMessage);
    }
  }

  async function sendMessage(
    conversationId: string,
    text: string,
    recipientDid: string,
    senderAccountId: string
  ) {
    // DM disappearing-messages timer is keyed by peer DID in app-core's store
    // (save/load_conversation_expiry), so read it with recipientDid — not the
    // full conversation id — to match what the wire envelope is stamped with.
    const timer =
      (await serviceFor(senderAccountId).getConversationTimer(recipientDid).catch(() => null)) ?? 0;
    await sendOptimistic(
      conversationId,
      text,
      senderAccountId,
      timer,
      (sentAtMs) =>
        serviceFor(senderAccountId).sendDm(
          recipientDid,
          Array.from(new TextEncoder().encode(text)),
          sentAtMs
        ),
      "Send failed"
    );
  }

  async function sendGroupMessage(conversation: Conversation, text: string) {
    if (!conversation.groupId) return;
    const groupId = conversation.groupId;
    const svc = serviceFor(conversation.accountId);
    const timer = (await svc.groupExpirySeconds(groupId).catch(() => 0)) ?? 0;
    await sendOptimistic(
      conversation.id,
      text,
      conversation.accountId,
      timer,
      (sentAtMs) => svc.sendGroupMessage(groupId, Array.from(new TextEncoder().encode(text)), sentAtMs),
      "Group send failed"
    );
  }

  // Send a message carrying attachments and/or link previews to either a DM or a
  // group — one path for both targets (mirrors iOS sendWithAttachments). Routes
  // through sendOptimistic so the local row, persistence, and delivery states
  // behave identically to a plain send. The body may be empty (attachment-only).
  async function sendMessageWithAttachments(
    conversation: Conversation,
    text: string,
    attachments: AttachmentFfi[],
    previews: LinkPreviewFfi[]
  ) {
    const target = messageTargetFor(conversation);
    const svc = serviceFor(conversation.accountId);
    const timer =
      target.type === "group"
        ? (await svc.groupExpirySeconds(target.group_id).catch(() => 0)) ?? 0
        : (await svc.getConversationTimer(target.recipient_did).catch(() => null)) ?? 0;
    await sendOptimistic(
      conversation.id,
      text,
      conversation.accountId,
      timer,
      (sentAtMs) =>
        svc.sendMessageWithAttachments(target, text, attachments, previews, sentAtMs),
      "Send failed",
      attachments,
      previews
    );
  }

  // ── Attachments / link previews / external links (thin service pass-throughs,
  // kept on the context so views never reach the service directly and Mock mode
  // keeps working) ─────────────────────────────────────────────────────────────
  // Attachments encrypt/decrypt with the owning account's keys, so they route to
  // that account's core (accountId threaded from the conversation in the view).
  function uploadAttachment(
    accountId: string,
    plaintext: number[],
    contentType: string,
    fileName: string | null,
    width: number,
    height: number,
    durationMs: number,
    thumbnail: number[],
    flags: number
  ): Promise<AttachmentFfi> {
    return serviceFor(accountId).uploadAttachment(
      plaintext,
      contentType,
      fileName,
      width,
      height,
      durationMs,
      thumbnail,
      flags
    );
  }

  function downloadAttachment(accountId: string, attachment: AttachmentFfi): Promise<number[]> {
    return serviceFor(accountId).downloadAttachment(attachment);
  }

  // Link-preview fetch + external-open are pure Rust commands (no core) → account-less.
  function fetchLinkPreview(url: string): Promise<LinkPreviewMetaFfi> {
    return onboardingService().fetchLinkPreview(url);
  }

  function openExternal(url: string): Promise<void> {
    return onboardingService().openExternal(url);
  }

  function markAllMessagesRead(conversationId: string, accountId: string) {
    // Optimistically clear the seeded unread badge on open (A5). Also flips the
    // reactive dep so the chat-list badge re-renders as the transcript loads.
    const ci = store.conversations.findIndex((c) => c.id === conversationId);
    if (ci >= 0 && store.conversations[ci].unreadCount) {
      setStore("conversations", ci, "unreadCount", 0);
    }
    const msgs = store.messagesByConversation[conversationId];
    if (!msgs) return;
    const now = Date.now();
    let changed = false;
    // Collect the sent-at timestamps of inbound messages that newly flip to
    // read, so we can acknowledge them to the sender via a read receipt.
    const newlyReadSentAt: number[] = [];
    const updated = msgs.map((m) => {
      if (m.readAtMs === undefined && m.senderAccountId !== accountId) {
        changed = true;
        newlyReadSentAt.push(m.sentAtMs);
        return { ...m, readAtMs: now };
      }
      return m;
    });
    if (changed) {
      setStore("messagesByConversation", conversationId, updated);
      void serviceFor(accountId)
        .markMessagesRead(conversationId, now)
        .catch((e: unknown) => {
          console.warn("markMessagesRead failed:", e);
        });
      // Send a read receipt to the DM partner so their bubbles flip to "read".
      // Receipts are 1:1 (a single recipient), so this applies to DMs only —
      // a group has no single recipient. Suppress for un-accepted requests
      // (opening to evaluate isn't acknowledgement) and for blocked contacts
      // (never signal "read" to someone you cut off) — don't rely on app-core
      // to gate it.
      const conv = store.conversations.find((c) => c.id === conversationId);
      if (
        conv &&
        !conv.isGroup &&
        !conv.isRequest &&
        !conv.isBlocked &&
        conv.recipientDid &&
        newlyReadSentAt.length > 0
      ) {
        const recipientDid = conv.recipientDid;
        void serviceFor(accountId)
          .sendReadReceipt(recipientDid, newlyReadSentAt)
          .catch((e: unknown) => {
            console.warn("sendReadReceipt failed:", e);
          });
      }
    }
  }

  function messageTargetFor(conversation: Conversation): MessageTarget {
    return conversation.isGroup && conversation.groupId
      ? { type: "group", group_id: conversation.groupId }
      : { type: "dm", recipient_did: conversation.recipientDid ?? "" };
  }

  function unreadCount(conversation: Conversation): number {
    // Read messagesByConversation unconditionally so this accessor always tracks
    // it reactively — otherwise a conversation opened with 0 unread never
    // establishes the dependency and its badge goes stale when a later message
    // arrives. Once the transcript is loaded, per-message read state is
    // authoritative (and reflects optimistic clears); before that, use the count
    // seeded from ConversationSummaryFfi.unreadCount (core excludes own +
    // expired). Mirrors iOS unreadCount(for:).
    const msgs = store.messagesByConversation[conversation.id];
    if (guards.loadedMessages.has(conversation.id)) {
      return (msgs ?? []).filter(
        (m) => m.readAtMs === undefined && m.senderAccountId !== conversation.accountId
      ).length;
    }
    return conversation.unreadCount ?? 0;
  }

  // ── Track A: reactions / edit / delete / retry ─────────────────────────────

  function reactionsFor(
    conversation: Conversation,
    message: Message
  ): ReactionFfi[] {
    const all = store.reactionsByConversation[conversation.id] ?? [];
    return all.filter(
      (r) =>
        r.targetAuthor === message.senderAccountId &&
        r.targetSentAtMs === message.sentAtMs
    );
  }

  function loadReactions(conversationId: string) {
    if (guards.loadedReactions.has(conversationId)) return;
    const accountId = accountIdForConversation(conversationId);
    if (accountId === null) return;
    guards.loadedReactions.add(conversationId);
    void serviceFor(accountId)
      .loadReactions(conversationId)
      .then((rows) => {
        setStore("reactionsByConversation", conversationId, rows);
      })
      .catch((err: unknown) => {
        console.warn("loadReactions failed for", conversationId, err);
        guards.loadedReactions.delete(conversationId);
      });
  }

  function toggleReaction(
    conversation: Conversation,
    message: Message,
    emoji: string
  ) {
    const myDid = conversation.accountId;
    const convId = conversation.id;
    const targetAuthor = message.senderAccountId;
    const targetSentAtMs = message.sentAtMs;
    const now = Date.now();

    const current = store.reactionsByConversation[convId] ?? [];
    const existingMine = current.find(
      (r) =>
        r.targetAuthor === targetAuthor &&
        r.targetSentAtMs === targetSentAtMs &&
        r.reactorDid === myDid
    );
    const remove = existingMine?.emoji === emoji;

    // Optimistic in-memory update: drop my prior reaction on this message,
    // then (unless toggling the same emoji off) add the new one.
    const withoutMine = current.filter(
      (r) =>
        !(
          r.targetAuthor === targetAuthor &&
          r.targetSentAtMs === targetSentAtMs &&
          r.reactorDid === myDid
        )
    );
    const next = remove
      ? withoutMine
      : [
          ...withoutMine,
          {
            conversationId: convId,
            targetAuthor,
            targetSentAtMs,
            reactorDid: myDid,
            emoji,
            reactedAtMs: now,
          },
        ];
    setStore("reactionsByConversation", convId, next);

    void serviceFor(conversation.accountId)
      .sendReaction(
        messageTargetFor(conversation),
        targetAuthor,
        targetSentAtMs,
        emoji,
        remove,
        now
      )
      .catch((e: unknown) => {
        console.warn("sendReaction failed:", e);
      });
  }

  function editMessage(
    conversation: Conversation,
    message: Message,
    newBody: string
  ) {
    const trimmed = newBody.trim();
    if (!trimmed || trimmed === message.body) return;
    const now = Date.now();
    setStore("messagesByConversation", conversation.id, (prev) =>
      (prev ?? []).map((m) =>
        m.id === message.id
          ? {
              ...m,
              body: trimmed,
              editedAtMs: now,
              editCount: m.editCount + 1,
            }
          : m
      )
    );
    // Keep the sidebar preview in sync when the edited message is the latest
    // (the inbound messageEdited handler does the same for peers' edits).
    const convIdx = store.conversations.findIndex((c) => c.id === conversation.id);
    if (
      convIdx >= 0 &&
      store.conversations[convIdx]?.lastMessageDate === message.sentAtMs
    ) {
      const preview = trimmed.length > 100 ? trimmed.slice(0, 100) + "…" : trimmed;
      setStore("conversations", convIdx, "lastMessage", preview);
    }
    void serviceFor(conversation.accountId)
      .sendEdit(messageTargetFor(conversation), message.sentAtMs, trimmed, now)
      .catch((e: unknown) => {
        console.warn("sendEdit failed:", e);
      });
  }

  function loadMessageRevisions(
    conversation: Conversation,
    message: Message
  ): Promise<MessageRevisionFfi[]> {
    return serviceFor(conversation.accountId)
      .loadMessageRevisions(
        conversation.id,
        message.senderAccountId,
        message.sentAtMs
      )
      .catch((e: unknown) => {
        console.warn("loadMessageRevisions failed:", e);
        return [] as MessageRevisionFfi[];
      });
  }

  function clearReactionsForMessage(
    conversationId: string,
    targetAuthor: string,
    targetSentAtMs: number
  ) {
    const current = store.reactionsByConversation[conversationId];
    if (!current) return;
    setStore(
      "reactionsByConversation",
      conversationId,
      current.filter(
        (r) =>
          !(
            r.targetAuthor === targetAuthor &&
            r.targetSentAtMs === targetSentAtMs
          )
      )
    );
  }

  function deleteMessage(
    conversation: Conversation,
    message: Message,
    forEveryone: boolean
  ) {
    const now = Date.now();
    if (forEveryone) {
      setStore("messagesByConversation", conversation.id, (prev) =>
        (prev ?? []).map((m) =>
          m.id === message.id
            ? { ...m, body: "", isDeleted: true, editedAtMs: undefined }
            : m
        )
      );
    } else {
      setStore("messagesByConversation", conversation.id, (prev) =>
        (prev ?? []).filter((m) => m.id !== message.id)
      );
    }
    clearReactionsForMessage(
      conversation.id,
      message.senderAccountId,
      message.sentAtMs
    );
    void serviceFor(conversation.accountId)
      .sendDelete(
        messageTargetFor(conversation),
        message.senderAccountId,
        message.sentAtMs,
        forEveryone,
        now
      )
      .catch((e: unknown) => {
        console.warn("sendDelete failed:", e);
      });
  }

  async function retryMessage(conversation: Conversation, message: Message) {
    // Flip back to "sending", re-run the transport with a fresh timestamp, and
    // resolve to sent/failed exactly like the original optimistic send.
    const sentAtMs = Date.now();
    setStore("messagesByConversation", conversation.id, (prev) =>
      (prev ?? []).map((m) =>
        m.id === message.id
          ? { ...m, deliveryStatus: DeliveryStatus.sending, sentAtMs }
          : m
      )
    );
    // Persist each state (same contract as the original optimistic send) so the
    // retried message survives a reload regardless of outcome. Matches iOS.
    const svc = serviceFor(conversation.accountId);
    const persist = (status: DeliveryStatus) =>
      svc.saveMessage(
        buildStoredMessage({
          id: message.id,
          conversationId: conversation.id,
          senderDid: message.senderAccountId,
          body: message.body,
          sentAtMs,
          readAtMs: sentAtMs,
          deliveryStatus: status,
          editedAtMs: message.editedAtMs ?? null,
          editCount: message.editCount,
          deleted: message.isDeleted,
          kind: message.kind,
          metadata: message.metadata ?? null,
          expireTimerSecs: message.expireTimerSecs,
        })
      );
    await persist(DeliveryStatus.sending);
    const bytes = Array.from(new TextEncoder().encode(message.body));
    try {
      if (conversation.isGroup && conversation.groupId) {
        await svc.sendGroupMessage(conversation.groupId, bytes, sentAtMs);
      } else if (conversation.recipientDid) {
        await svc.sendDm(conversation.recipientDid, bytes, sentAtMs);
      } else {
        throw new Error("no transport target");
      }
      setStore("messagesByConversation", conversation.id, (prev) =>
        (prev ?? []).map((m) =>
          m.id === message.id
            ? { ...m, deliveryStatus: DeliveryStatus.sent }
            : m
        )
      );
      void persist(DeliveryStatus.sent).catch((err: unknown) => {
        console.warn("saveMessage after retry failed:", err);
      });
    } catch (e) {
      setStore("messagesByConversation", conversation.id, (prev) =>
        (prev ?? []).map((m) =>
          m.id === message.id
            ? { ...m, deliveryStatus: DeliveryStatus.failed }
            : m
        )
      );
      void persist(DeliveryStatus.failed).catch((err: unknown) => {
        console.warn("saveMessage (failed) after retry failed:", err);
      });
      console.warn("retryMessage failed:", e);
    }
  }

  return {
    sendMessage,
    sendGroupMessage,
    sendMessageWithAttachments,
    uploadAttachment,
    downloadAttachment,
    fetchLinkPreview,
    openExternal,
    loadMessagesFromStore,
    markAllMessagesRead,
    unreadCount,
    reactionsFor,
    loadReactions,
    toggleReaction,
    editMessage,
    loadMessageRevisions,
    deleteMessage,
    retryMessage,
    reloadMessagesIfLoaded,
    clearReactionsForMessage,
  };
}
