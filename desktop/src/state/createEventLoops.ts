import { createMemo, onCleanup, type Setter } from "solid-js";
import type { SetStoreFunction } from "solid-js/store";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import type { Conversation } from "../models";
import { attachmentPlaceholder } from "../lib/format";
import { DeliveryStatus, type Message } from "../models/Message";
import type {
  ConnectionState,
  IncomingEvent,
} from "../services/AvalancheService";
import { deliveryRank } from "../lib/format";
import { buildStoredMessage } from "./helpers";
import type { Services } from "./createServices";
import type { AppContextValue, AppStore, SessionGuards } from "./types";

export interface EventLoopsDeps {
  store: AppStore;
  setStore: SetStoreFunction<AppStore>;
  serviceFor: Services["serviceFor"];
  guards: SessionGuards;
  reloadConversations: () => Promise<void>;
  getServerUrl: (accountId: string) => string;
  cachedDisplayName: (did: string) => string | undefined;
  reloadMessagesIfLoaded: (cid: string) => void;
  clearReactionsForMessage: (
    conversationId: string,
    targetAuthor: string,
    targetSentAtMs: number
  ) => void;
  selectedConversationId: () => string | null;
  setGroupMetaChange: Setter<{ groupId: string; n: number }>;
}

// The TS-owned per-account event + connection loops, the inbound-event
// handlers they drain into, native notifications, and the derived aggregate
// connection state. Registers its own onCleanup(stopPolling) — the factory
// must be called synchronously in the AppProvider body so Solid ownership is
// correct (desktop/CLAUDE.md "TS owns the event loop").
export type EventLoops = Pick<
  AppContextValue,
  "aggregateConnectionState" | "reconnectNow"
> & {
  // Internal API for the other state modules (loop lifecycle is driven by
  // enterApp / resetSession / removeAccountLocally in createAccounts)
  startPollingFor: (accountId: string) => void;
  stopPollingFor: (accountId: string) => void;
  stopPolling: () => void;
};

export function createEventLoops(deps: EventLoopsDeps): EventLoops {
  const {
    store,
    setStore,
    serviceFor,
    guards,
    reloadConversations,
    getServerUrl,
    cachedDisplayName,
    reloadMessagesIfLoaded,
    clearReactionsForMessage,
    selectedConversationId,
    setGroupMetaChange,
  } = deps;

  // Event + connection loop lifecycle, one of each per account (mirrors iOS
  // eventTasks/stateTasks and Android eventJobs/stateJobs). A loop runs while its
  // accountId is in `loops`; teardown removes the id and clears any pending retry
  // timer. `startPollingFor` is idempotent (guards on map membership), so the
  // restore + add-account paths can both call it without spawning duplicate loops
  // that would process every event twice (desktop/CLAUDE.md "Background loops").
  type LoopHandle = {
    eventRunning: boolean;
    connRunning: boolean;
    eventTimeout?: ReturnType<typeof setTimeout>;
  };
  const loops = new Map<string, LoopHandle>();

  // Manual "Reconnect now" (offline banner). Best-effort — wakes every signed-in
  // account's reconnect loop (mirrors iOS, which drives connectivity per core).
  function reconnectNow() {
    for (const account of store.accounts) {
      void serviceFor(account.id)
        .reconnectNow()
        .catch(() => { /* signed out / unavailable */ });
    }
  }

  // ── Foreground hook ───────────────────────────────────────────────────────
  // On window focus, tell the core the app is active (T77). This wakes the
  // reconnect loop and probes a possibly-dead socket — so after the machine
  // resumes from sleep or a network change, a stale connection (and any
  // messages sent/read on another linked device meanwhile) recovers promptly
  // via the reconnect + storage resync, without a restart.
  //
  // We deliberately do NOT deactivate on blur, unlike iOS's scenePhase gating.
  // Desktop has no OS suspension and no push fallback, and close-to-tray
  // explicitly promises that messages/notifications keep arriving while the
  // window is hidden — so the keepalive (and its dead-socket detection) must
  // stay on whenever the process is running. The core defaults to active, so
  // never calling `setAppActive(false)` keeps the connection alive for the
  // app's lifetime; focus is purely an opportunistic reconnect trigger.
  // No-op before sign-in: the command short-circuits when there's no account.
  let focusUnlisten: (() => void) | undefined;
  getCurrentWindow()
    .onFocusChanged(({ payload: focused }) => {
      if (focused) {
        // Push foreground-active to every account (iOS setIsAppActive loops all
        // cores) so each one's keepalive + opportunistic reconnect fires.
        for (const account of store.accounts) {
          void serviceFor(account.id)
            .setAppActive(true)
            .catch(() => { /* offline / signed out */ });
        }
      }
    })
    .then((un) => { focusUnlisten = un; })
    .catch(() => { /* Tauri window API unavailable (browser/test) */ });
  onCleanup(() => focusUnlisten?.());

  // Fire a native notification for an inbound message (mirrors iOS
  // NotificationPresenter.present). Suppressed when the user is already viewing
  // this conversation in a focused window; shown otherwise (window unfocused, or
  // focused on a different conversation). Permission is requested on first use.
  async function maybeNotify(conversationId: string, senderDid: string, body: string) {
    const text = body.trim();
    if (!text) return;
    try {
      const focused = await getCurrentWindow().isFocused().catch(() => false);
      if (focused && selectedConversationId() === conversationId) return;
      let granted = await isPermissionGranted();
      if (!granted) granted = (await requestPermission()) === "granted";
      if (!granted) return;
      const title = cachedDisplayName(senderDid) || senderDid;
      const preview = text.length > 120 ? text.slice(0, 120) + "…" : text;
      sendNotification({ title, body: preview });
    } catch (e) {
      console.warn("notification failed:", e);
    }
  }

  // ── Event loop ────────────────────────────────────────────────────────────

  // Drain a batch of decrypted events (mirrors iOS `AppState.eventLoop`,
  // AppState.swift). The switch only *collects*; state is applied once after the
  // loop, so a whole batch triggers at most one conversation reload — this is
  // what kills the interleaving-reload races the old per-case reloads caused.
  // Point mutations that don't benefit from batching (edits, deletes) are
  // applied inline via small named handlers, matching iOS.
  // `accountId` is the account whose event loop delivered this batch — every
  // event here belongs to that account (used to attribute incoming messages /
  // new conversations to the right identity in the shared inbox).
  function handleIncomingEvents(events: IncomingEvent[], accountId: string) {
    const messages: Extract<IncomingEvent, { type: "message" }>["msg"][] = [];
    const receiptUpdates: Extract<
      IncomingEvent,
      { type: "receiptUpdate" }
    >["update"][] = [];
    let needsConversationReload = false;

    for (const ev of events) {
      switch (ev.type) {
        case "message":
          messages.push(ev.msg);
          break;
        case "receiptUpdate":
          receiptUpdates.push(ev.update);
          break;
        case "messageEdited": {
          const e = ev as Extract<IncomingEvent, { type: "messageEdited" }>;
          applyInboundEdit(
            e.conversation_id ?? "",
            e.author_did,
            e.sent_at_ms,
            e.new_body,
            e.edited_at_ms
          );
          break;
        }
        case "messageDeleted": {
          const d = ev as Extract<IncomingEvent, { type: "messageDeleted" }>;
          applyInboundDelete(d.conversation_id ?? "", d.author_did, d.sent_at_ms);
          break;
        }
        case "reactionUpdated": {
          const r = ev as Extract<IncomingEvent, { type: "reactionUpdated" }>;
          applyInboundReaction(
            r.conversation_id,
            r.target_author,
            r.target_sent_at_ms,
            r.reactor_did,
            r.emoji,
            r.removed
          );
          break;
        }
        case "messagesExpired": {
          const exp = ev as Extract<IncomingEvent, { type: "messagesExpired" }>;
          for (const cid of exp.conversation_ids) reloadMessagesIfLoaded(cid);
          needsConversationReload = true;
          break;
        }
        case "groupMetadataChanged": {
          // app-core has already persisted a system row (e.g. "X made Y an
          // admin") for this change. Refresh the group's timeline (if loaded)
          // so it appears, then refresh the conversation list/preview.
          const gm = ev as Extract<IncomingEvent, { type: "groupMetadataChanged" }>;
          reloadMessagesIfLoaded(`group-${gm.event.groupId}`);
          // Notify any open ConversationView for this group to re-check
          // membership (e.g. you were removed by another admin while viewing).
          setGroupMetaChange((p) => ({ groupId: gm.event.groupId, n: p.n + 1 }));
          needsConversationReload = true;
          break;
        }
        case "conversationUpdated": {
          // A `SyncSent`/`SyncRead` transcript from another of my own linked
          // devices changed exactly this conversation's stored content (a
          // message I sent, an edit/delete/reaction I made, or read-state I
          // cleared). Re-read just this timeline so it surfaces live, then
          // refresh the chat-list preview. Mirrors iOS `conversationUpdated`.
          const cu = ev as Extract<IncomingEvent, { type: "conversationUpdated" }>;
          reloadMessagesIfLoaded(cu.conversation_id);
          needsConversationReload = true;
          break;
        }
        case "groupInvite":
        case "storageSynced":
          needsConversationReload = true;
          break;
        default:
          console.warn(
            "handleIncomingEvents: unknown event type",
            (ev as { type: string }).type
          );
          break;
      }
    }

    // Apply phase — run once for the whole batch, attributed to this loop's
    // account.
    for (const m of messages) handleIncomingMessage(m, accountId);
    if (receiptUpdates.length) applyDeliveryStatusUpdates(receiptUpdates);
    if (needsConversationReload) void reloadConversations();
  }

  // Append or reconcile a single decrypted message (iOS `handleIncomingMessage`).
  function handleIncomingMessage(
    m: Extract<IncomingEvent, { type: "message" }>["msg"],
    accountId: string
  ) {
    const conversationId = m.groupId
      ? `group-${m.groupId}`
      : `dm-${accountId}-${m.senderDid}`;
    const senderIsSelf = m.senderDid === accountId;

    if (senderIsSelf && m.sentAtMs !== null) {
      // Echo of our own outgoing message — update the optimistic entry in-place
      // by sentAtMs instead of appending a duplicate. Only match messages still
      // in a non-terminal delivery state (sending/sent); a delivered message is
      // already confirmed and should not be matched again.
      setStore("messagesByConversation", conversationId, (prev) =>
        (prev ?? []).map((existing) =>
          existing.sentAtMs === m.sentAtMs &&
          existing.senderAccountId === accountId &&
          (existing.deliveryStatus === DeliveryStatus.sending ||
            existing.deliveryStatus === DeliveryStatus.sent)
            ? {
                ...existing,
                deliveryStatus: DeliveryStatus.delivered,
                id: `server-${m.serverId}`,
              }
            : existing
        )
      );
      return;
    }

    // Received from another user — append as a new message.
    const body = new TextDecoder().decode(new Uint8Array(m.plaintext));
    const msg: Message = {
      id: crypto.randomUUID(),
      conversationId,
      senderAccountId: m.senderDid,
      body,
      sentAtMs: m.sentAtMs ?? Date.now(),
      deliveryStatus: DeliveryStatus.delivered,
      editCount: 0,
      isDeleted: false,
      kind: 0,
      expireTimerSecs: m.expireTimerSecs,
      attachments: m.attachments,
      previews: m.previews,
    };
    setStore("messagesByConversation", conversationId, (prev) => [
      ...(prev ?? []),
      msg,
    ]);
    // Chat-list preview: real text, else "Photo"/"Attachment" for an
    // attachment-only message (iOS parity).
    const previewSource =
      body.trim().length > 0
        ? body
        : attachmentPlaceholder(m.attachments[0]?.contentType);
    const previewText =
      previewSource.length > 100 ? previewSource.slice(0, 100) + "…" : previewSource;
    // Seed/bump the unread badge for conversations whose transcript isn't loaded
    // (the loaded case is counted from per-message read state). (A5)
    if (!guards.loadedMessages.has(conversationId)) {
      const ci = store.conversations.findIndex((c) => c.id === conversationId);
      if (ci >= 0) {
        setStore("conversations", ci, "unreadCount", (n) => (n ?? 0) + 1);
      }
    }
    // Fire a native notification for the inbound message (suppressed when the
    // user is already viewing this conversation in a focused window).
    void maybeNotify(conversationId, m.senderDid, body);
    // Persist the incoming message. app-core does NOT persist messages on the
    // receive path (the client owns local history), so without this every
    // received message is lost on app restart/refresh while sent messages
    // (which are saved) survive. readAtMs stays null (unread) until the
    // conversation is opened; the store starts the disappearing-messages
    // countdown on read. Mirrors iOS AppState.
    void serviceFor(accountId)
      .saveMessage(
        buildStoredMessage({
          id: msg.id,
          conversationId,
          senderDid: m.senderDid,
          body,
          sentAtMs: msg.sentAtMs,
          readAtMs: null,
          deliveryStatus: msg.deliveryStatus,
          expireTimerSecs: m.expireTimerSecs,
          attachments: m.attachments,
          previews: m.previews,
        })
      )
      .catch((err: unknown) => {
        console.warn("saveMessage (incoming) failed:", err);
      });

    // Update conversation preview, or create the conversation in-memory.
    const convIdx = store.conversations.findIndex((c) => c.id === conversationId);
    if (convIdx >= 0) {
      setStore("conversations", convIdx, "lastMessage", previewText);
      setStore(
        "conversations",
        convIdx,
        "lastMessageDate",
        m.sentAtMs ?? Date.now()
      );
      // Clear stale group system-event fields (see sendOptimistic).
      setStore("conversations", convIdx, "lastMessageKind", 0);
      setStore("conversations", convIdx, "lastMessageMetadata", undefined);
    } else {
      // Conversation not in the list yet — create it in-memory. The incoming
      // message is now persisted (above), so it will also show up in the DB
      // summaries on the next reload; tracking it pending bridges the gap.
      const isGroup = !!m.groupId;
      // A DM from a sender that didn't pass the message-request gate is a
      // request (docs/12 §1). app-core reports the verdict but leaves
      // persistence to us — flag it so the request banner shows and survives a
      // refresh.
      const isRequest = !isGroup && m.isRequest;
      const serverUrl = getServerUrl(accountId);
      const newConv: Conversation = {
        id: conversationId,
        title: isGroup ? "Group" : m.senderDid,
        accountId,
        serverUrl,
        recipientDid: isGroup ? undefined : m.senderDid,
        groupId: m.groupId ?? undefined,
        lastMessage: previewText,
        lastMessageDate: m.sentAtMs ?? Date.now(),
        lastMessageKind: 0,
        isGroup,
        isRequest,
        isBlocked: false,
        // First message in a brand-new (unopened) conversation → 1 unread. (A5)
        unreadCount: 1,
      };
      guards.pendingConversations.add(conversationId);
      setStore("conversations", (prev) => [newConv, ...prev]);
      if (isRequest) {
        void serviceFor(accountId)
          .setPendingRequest(m.senderDid, true)
          .catch((e: unknown) => {
            console.warn("setPendingRequest failed:", e);
          });
      }
    }
  }

  // Apply a batch of delivery-status receipts (iOS `applyDeliveryStatusUpdates`).
  // Receipts only ever move a message forward; `failed` applies only from a
  // non-terminal state (see `deliveryRank`).
  function applyDeliveryStatusUpdates(
    updates: Extract<IncomingEvent, { type: "receiptUpdate" }>["update"][]
  ) {
    for (const update of updates) {
      const msgs = store.messagesByConversation[update.conversationId];
      if (!msgs) continue;
      const incoming = (
        update.deliveryStatus >= 0 && update.deliveryStatus <= 4
          ? update.deliveryStatus
          : DeliveryStatus.sent
      ) as DeliveryStatus;
      setStore(
        "messagesByConversation",
        update.conversationId,
        msgs.map((m) => {
          if (m.sentAtMs !== update.sentAtMs) return m;
          if (incoming === DeliveryStatus.failed) {
            // Only apply `failed` when the message is still non-terminal
            // (sending/sent). A delivered or read message is never downgraded
            // to failed by a stale or out-of-order receipt.
            if (deliveryRank(m.deliveryStatus) <= deliveryRank(DeliveryStatus.sent)) {
              return { ...m, deliveryStatus: DeliveryStatus.failed };
            }
            return m;
          }
          // For normal forward states, only advance — never go backwards.
          if (deliveryRank(incoming) > deliveryRank(m.deliveryStatus)) {
            return { ...m, deliveryStatus: incoming };
          }
          return m;
        })
      );
    }
  }

  // Apply an inbound edit (iOS `applyInboundEdit`).
  // Matches the target by (senderAccountId, sentAtMs) — identical to iOS
  // applyInboundEdit. T66 (also match on serverId, to disambiguate two messages
  // that share a millisecond) is blocked: the messageEdited/messageDeleted wire
  // events carry no server_id (only conversation_id, author_did, sent_at_ms), so
  // there is nothing to match against. Closing this needs server_id added to
  // those events across the protocol + all platforms — a contract change to
  // raise with the maintainer, not a desktop-only edit.
  function applyInboundEdit(
    cid: string,
    authorDid: string,
    sentAtMs: number,
    newBody: string,
    editedAtMs: number
  ) {
    if (cid && store.messagesByConversation[cid]) {
      setStore("messagesByConversation", cid, (prev) =>
        (prev ?? []).map((m) =>
          m.senderAccountId === authorDid && m.sentAtMs === sentAtMs
            ? {
                ...m,
                body: newBody,
                editedAtMs,
                editCount: m.editCount + 1,
              }
            : m
        )
      );
      // Update conversation preview if the edited message was the most recent.
      const convIdx = store.conversations.findIndex((c) => c.id === cid);
      if (
        convIdx >= 0 &&
        store.conversations[convIdx]?.lastMessageDate === sentAtMs
      ) {
        const previewText =
          newBody.length > 100 ? newBody.slice(0, 100) + "..." : newBody;
        setStore("conversations", convIdx, "lastMessage", previewText);
      }
    } else {
      // Messages not loaded or no conversation_id — reload to pick up the edit.
      void reloadConversations();
    }
  }

  // Apply an inbound delete tombstone (iOS `applyInboundDelete`).
  function applyInboundDelete(cid: string, authorDid: string, sentAtMs: number) {
    if (cid && store.messagesByConversation[cid]) {
      setStore("messagesByConversation", cid, (prev) =>
        (prev ?? []).map((m) =>
          m.senderAccountId === authorDid && m.sentAtMs === sentAtMs
            ? { ...m, isDeleted: true }
            : m
        )
      );
      // A deleted message drops its reactions too.
      clearReactionsForMessage(cid, authorDid, sentAtMs);
      // Update conversation preview if the deleted message was the most recent.
      const convIdx = store.conversations.findIndex((c) => c.id === cid);
      if (
        convIdx >= 0 &&
        store.conversations[convIdx]?.lastMessageDate === sentAtMs
      ) {
        setStore("conversations", convIdx, "lastMessage", "[deleted]");
      }
    } else {
      // Messages not loaded or no conversation_id — reload to pick up the tombstone.
      void reloadConversations();
    }
  }

  // Apply an inbound reaction add/remove (iOS `applyInboundReaction`). Replaces
  // any prior reaction by the same reactor on the target message, then re-adds
  // it unless this was a removal.
  function applyInboundReaction(
    cid: string,
    targetAuthor: string,
    targetSentAtMs: number,
    reactorDid: string,
    emoji: string,
    removed: boolean
  ) {
    const current = store.reactionsByConversation[cid] ?? [];
    const withoutReactor = current.filter(
      (x) =>
        !(
          x.targetAuthor === targetAuthor &&
          x.targetSentAtMs === targetSentAtMs &&
          x.reactorDid === reactorDid
        )
    );
    const next = removed
      ? withoutReactor
      : [
          ...withoutReactor,
          {
            conversationId: cid,
            targetAuthor,
            targetSentAtMs,
            reactorDid,
            emoji,
            reactedAtMs: Date.now(),
          },
        ];
    setStore("reactionsByConversation", cid, next);
  }

  function loopHandle(accountId: string): LoopHandle {
    let h = loops.get(accountId);
    if (!h) {
      h = { eventRunning: false, connRunning: false };
      loops.set(accountId, h);
    }
    return h;
  }

  // Per-account event loop. `nextEvents()` blocks until decrypted events arrive
  // for THIS account (WebSocket push via app-core's channel), then drains the
  // batch attributed to it. The Tauri command is async — it parks on the tokio
  // runtime, so the JS event loop stays responsive. Idempotent: guarded by the
  // per-account handle so a second startPollingFor (restore + add-account both
  // call it) never doubles the loop and processes every event twice.
  function startEventLoopFor(accountId: string) {
    if (!accountId) return;
    const h = loopHandle(accountId);
    if (h.eventRunning) return;
    h.eventRunning = true;

    const svc = serviceFor(accountId);
    const loop = async () => {
      if (!h.eventRunning) return;
      try {
        const events = await svc.nextEvents();
        // A poll in flight when the account was torn down (logout / leave /
        // delete) still resolves here; re-check before applying so we never
        // attribute events to a removed account (mirrors iOS's post-await
        // `guard !Task.isCancelled`). The old single-loop code relied on
        // getSoleAccountId() returning null for this; the per-account loops
        // need the explicit check.
        if (!h.eventRunning) return;
        handleIncomingEvents(events, accountId);
        if (h.eventRunning) void loop();
      } catch {
        if (h.eventRunning) {
          h.eventTimeout = setTimeout(() => void loop(), 1000);
        }
      }
    };
    void loop();
  }

  // Per-account connection loop (mirrors iOS `connectionStateLoop`): seed from
  // the current snapshot, then block on `waitForConnectionStateChange` and copy
  // each state the core emits into `connectionStates[accountId]`. The Rust core
  // owns reconnection and surfaces its progress as `reconnecting` / `connected`
  // states, so this loop is a thin mirror — connectivity churn flows through as
  // ConnectionState values, NOT as JS-side retries. A throw means the core/
  // session is gone, which is terminal.
  function startConnectionLoopFor(accountId: string) {
    if (!accountId) return;
    const h = loopHandle(accountId);
    if (h.connRunning) return;
    h.connRunning = true;

    const svc = serviceFor(accountId);
    const loop = async (last: ConnectionState) => {
      while (h.connRunning) {
        let next: ConnectionState;
        try {
          next = await svc.waitForConnectionStateChange(last);
        } catch {
          h.connRunning = false;
          break;
        }
        if (!h.connRunning) break;
        last = next;
        setStore("connectionStates", accountId, next);
      }
    };

    void svc
      .connectionState()
      .then((state) => {
        setStore("connectionStates", accountId, state);
        void loop(state);
      })
      .catch(() => {
        h.connRunning = false;
      });
  }

  function startPollingFor(accountId: string) {
    startEventLoopFor(accountId);
    startConnectionLoopFor(accountId);
  }

  // Tear down ONE account's loops (leave / delete / remove). Clears the
  // per-account handle so a later startPollingFor cleanly restarts it.
  function stopPollingFor(accountId: string) {
    const h = loops.get(accountId);
    if (!h) return;
    h.eventRunning = false;
    h.connRunning = false;
    if (h.eventTimeout) {
      clearTimeout(h.eventTimeout);
      h.eventTimeout = undefined;
    }
    loops.delete(accountId);
  }

  // Tear down every account's loops (full logout / provider unmount).
  function stopPolling() {
    for (const accountId of Array.from(loops.keys())) stopPollingFor(accountId);
  }

  onCleanup(stopPolling);

  // ── Derived: aggregate connection state ───────────────────────────────────

  const aggregateConnectionState = createMemo((): ConnectionState => {
    const states = Object.values(store.connectionStates);
    // No connection states yet means no accounts have connected — report
    // disconnected so the UI doesn't show a misleading "connected" indicator
    // before any connection exists.
    if (states.length === 0) return { type: "disconnected" };
    if (states.every((s) => s.type === "connected")) return { type: "connected" };
    for (const s of states) {
      if (s.type === "reconnecting") return s;
    }
    const any = states.find((s) => s.type !== "connected");
    return any ?? { type: "connected" };
  });

  return {
    startPollingFor,
    stopPollingFor,
    stopPolling,
    aggregateConnectionState,
    reconnectNow,
  };
}
