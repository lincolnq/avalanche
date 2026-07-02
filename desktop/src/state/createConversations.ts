import { createStore, reconcile, type SetStoreFunction } from "solid-js/store";
import type { Conversation } from "../models";
import { attachmentPlaceholder } from "../lib/format";
import { parseGroupEventMeta } from "../lib/groupEvents";
import type {
  ConversationSummaryFfi,
} from "../services/AvalancheService";
import {
  isGroupSummary,
  recipientDidFromConvId,
  parseDeepLink,
  decodeInviteToken,
  trimSlashes,
} from "./helpers";
import type { Services } from "./createServices";
import type { AppContextValue, AppStore, SessionGuards } from "./types";

export interface ConversationsDeps {
  store: AppStore;
  setStore: SetStoreFunction<AppStore>;
  serviceFor: Services["serviceFor"];
  guards: SessionGuards;
  setSelectedConversationId: (id: string | null) => void;
}

// Conversation-list building (merged inbox), the reactive display-name / is-bot
// caches, DM/group conversation materialization, and deep-link routing.
// The Pick keys are this module's slice of the context surface — typing them
// via Pick means a signature drift or dropped key errors here, at the factory,
// not at a distant view's destructure.
export type Conversations = Pick<
  AppContextValue,
  | "loadConversationsFromStore"
  | "reloadConversations"
  | "findOrCreateDMConversation"
  | "displayName"
  | "isBot"
  | "isDeepLink"
  | "handleDeepLink"
> & {
  // Internal API for the other state modules
  accountIdForConversation: (conversationId: string) => string | null;
  getServerUrl: (accountId: string) => string;
  findOrCreateGroupConversation: (
    groupId: string,
    title: string,
    accountId: string
  ) => Conversation;
  cachedDisplayName: (did: string) => string | undefined;
  resetCaches: () => void;
};

export function createConversations(deps: ConversationsDeps): Conversations {
  const { store, setStore, serviceFor, guards, setSelectedConversationId } = deps;

  // Reactive display-name cache: reads are tracked by Solid so components
  // re-render when a resolved name arrives.  A separate plain Set tracks
  // in-flight fetches to prevent duplicate IPC calls per DID.
  const [displayNameCache, setDisplayNameCache] = createStore<Record<string, string>>({});
  const displayNamePending: Set<string> = new Set();

  // Reactive is-bot cache, same pattern as displayNameCache: components read it
  // in a tracking scope so a bot avatar (hexagon) resolves once getAccountInfo
  // returns. A plain Set guards against duplicate in-flight fetches per DID.
  const [isBotCache, setIsBotCache] = createStore<Record<string, boolean>>({});
  const isBotPending: Set<string> = new Set();

  // Coalesces forced conversation reloads (the inbound-event handlers plus
  // safety/group actions) so their store reconciles don't interleave. A reload
  // requested while one is in flight queues exactly one follow-up rather than
  // launching a second interleaving load.
  let reloadInFlight: Promise<void> | null = null;
  let reloadQueued = false;

  // Resolve the owning accountId of a conversation already in the store. Used by
  // the few context methods that receive only a conversationId (not the whole
  // Conversation) to route per-account service calls. Returns null if unknown.
  function accountIdForConversation(conversationId: string): string | null {
    return store.conversations.find((c) => c.id === conversationId)?.accountId ?? null;
  }

  function getServerUrl(accountId: string): string {
    // Uses `servers[0]`, the server the account was created with / joined first,
    // which stands in for the account's discovery/home server until multi-server
    // is modeled. There is no `isCurrent` field on ServerInfo (the original code
    // that referenced one was a bug that silently returned "").
    // TODO(multi-server): per docs/53, an identity has one discovery/home server
    // plus additional memberships; there is no single user-selected "current
    // server". When multi-server lands, account-level calls should use the
    // discovery server and group/DM operations should use the conversation's own
    // server, rather than a global current-server flag.
    return (
      store.accounts
        .find((a) => a.id === accountId)
        ?.servers[0]?.url ?? ""
    );
  }

  // DIDs whose display names should be warmed from local storage before the
  // chat list renders (T78, mirrors iOS displayNameDidsToWarm): DM peers, group
  // last-message senders, and the actor/target of a group system-event preview.
  function displayNameDidsToWarm(
    summaries: ConversationSummaryFfi[],
    accountId: string
  ): string[] {
    const dids = new Set<string>();
    for (const s of summaries) {
      if (isGroupSummary(s)) {
        const last = s.lastMessage;
        if (!last) continue;
        if (last.senderDid) dids.add(last.senderDid);
        if (last.kind > 0) {
          // System-event previews resolve actor/target DIDs (e.g. "Alice made
          // Bob an admin"), so warm those too.
          const m = parseGroupEventMeta(last.metadata ?? undefined);
          if (m?.actor_did) dids.add(m.actor_did);
          if (m?.target_did) dids.add(m.target_did);
        }
      } else {
        const recipientDid = recipientDidFromConvId(s.conversationId, accountId);
        if (recipientDid) dids.add(recipientDid);
      }
    }
    return Array.from(dids);
  }

  async function loadConversationsFromStore() {
    if (guards.loadedConversations.value) return;
    guards.loadedConversations.value = true;

    if (store.accounts.length === 0) {
      // No account signed in — reset the guard so a later load (once an account
      // enters) isn't permanently suppressed.
      guards.loadedConversations.value = false;
      setStore("conversations", []);
      return;
    }

    // Build the merged inbox across every signed-in account (shared-inbox model).
    // Group conversation ids (`group-<groupId>`) are NOT account-scoped — matching
    // iOS/Android — so a group two of your identities both belong to dedups to a
    // single row owned by the first account that materializes it (`groupSeen`).
    // DM ids embed the owning account (`dm-<accountId>-<peer>`) so they're already
    // globally unique. Each account's rows are loaded from that account's own core.
    // Fetch every account's summaries (and warm its name cache) concurrently —
    // the loads are independent per core, so N accounts cost ~1× latency, not N×.
    // Processing below stays in account order so the group dedup is deterministic.
    const accountsList = store.accounts.slice();
    const perAccount = await Promise.all(
      accountsList.map(async (account) => {
        const accountId = account.id;
        const summaries = await serviceFor(accountId)
          .loadConversations()
          .catch(() => [] as ConversationSummaryFfi[]);
        // Warm the display-name cache from this account's local store (no network)
        // before building titles — avoids DM rows flashing the raw DID on cold
        // launch (T78, mirrors iOS displayNameDidsToWarm + cachedDisplayNames).
        const warmDids = displayNameDidsToWarm(summaries, accountId);
        if (warmDids.length) {
          try {
            const names = await serviceFor(accountId).cachedDisplayNames(warmDids);
            for (const [did, name] of Object.entries(names)) {
              if (name) setDisplayNameCache(did, name);
            }
          } catch {
            // Local-only warm; a failure just means rows resolve via the async path.
          }
        }
        return { account, summaries };
      })
    );

    const all: Conversation[] = [];
    const groupSeen = new Set<string>();
    for (const { account, summaries } of perAccount) {
      const accountId = account.id;
      const serverUrl = getServerUrl(accountId);
      for (const s of summaries) {
        const isGroup = isGroupSummary(s);
        // Dedup shared groups across accounts: first account to surface it wins.
        if (isGroup && groupSeen.has(s.conversationId)) continue;
        if (isGroup) groupSeen.add(s.conversationId);

        const groupId = s.conversationId.startsWith("group-")
          ? s.conversationId.slice("group-".length)
          : undefined;
        const recipientDid = !isGroup
          ? recipientDidFromConvId(s.conversationId, accountId) ?? undefined
          : undefined;
        const title = isGroup
          ? s.groupTitle ?? "Group"
          : displayNameCache[recipientDid ?? ""] ?? recipientDid ?? s.conversationId;

        // Caption-less attachment messages have an empty body — preview them as
        // "Photo"/"Attachment" using the summary's attachment content type (iOS
        // chat-list parity).
        const lastBody = s.lastMessage?.body ?? "";
        const lastPreview =
          lastBody.trim().length === 0 && s.lastMessageAttachmentContentType
            ? attachmentPlaceholder(s.lastMessageAttachmentContentType)
            : s.lastMessage?.body ?? undefined;

        all.push({
          id: s.conversationId,
          title,
          accountId,
          serverUrl,
          recipientDid,
          groupId,
          lastMessage: lastPreview,
          lastMessageDate: s.lastMessage?.sentAtMs ?? undefined,
          lastMessageKind: s.lastMessage?.kind ?? 0,
          lastMessageMetadata: s.lastMessage?.metadata ?? undefined,
          lastMessageSenderDid: s.lastMessage?.senderDid ?? undefined,
          isGroup,
          isRequest: s.isRequest,
          isBlocked: s.isBlocked,
          // Authoritative unread seed from core (excludes own + expired). (A5)
          unreadCount: s.unreadCount,
        });
      }
    }

    const dbIds = new Set(all.map((c) => c.id));
    // A pending conversation that now appears in the DB is fully persisted —
    // stop tracking it so it follows normal DB-driven lifecycle from here on.
    for (const id of dbIds) guards.pendingConversations.delete(id);
    // Preserve only still-unpersisted in-memory conversations. Other DB-absent
    // entries (e.g. a group the DB dropped after leaving) are intentionally let go.
    const preserved = store.conversations.filter(
      (c) => !dbIds.has(c.id) && guards.pendingConversations.has(c.id)
    );
    const merged = [...all, ...preserved].sort(
      (a, b) => (b.lastMessageDate ?? 0) - (a.lastMessageDate ?? 0)
    );
    setStore("conversations", merged);
  }

  // Force a fresh conversation reload, bypassing the load-once guard. Used by
  // the inbound-event handlers (and later by safety/group actions). If a reload
  // is already running, queue exactly one follow-up rather than launching a
  // second interleaving load — this is the "please reload" path, distinct from
  // the load-once `loadConversationsFromStore`.
  function reloadConversations(): Promise<void> {
    if (reloadInFlight) {
      reloadQueued = true;
      return reloadInFlight;
    }
    guards.loadedConversations.value = false;
    reloadInFlight = loadConversationsFromStore().finally(() => {
      reloadInFlight = null;
      if (reloadQueued) {
        reloadQueued = false;
        void reloadConversations();
      }
    });
    return reloadInFlight;
  }

  function findOrCreateDMConversation(
    recipientDid: string,
    accountId: string
  ): Conversation {
    const existing = store.conversations.find(
      (c) => c.accountId === accountId && c.recipientDid === recipientDid
    );
    if (existing) return existing;

    const serverUrl = getServerUrl(accountId);
    const convId = `dm-${accountId}-${recipientDid}`;
    // Trigger async fetch; title updates reactively when the cache populates.
    const title = displayName(recipientDid, accountId);
    const conv: Conversation = {
      id: convId,
      title,
      accountId,
      serverUrl,
      recipientDid,
      isGroup: false,
      isRequest: false,
      isBlocked: false,
      lastMessageKind: 0,
    };
    // Track as pending so a reload (e.g. a storageSynced event firing before
    // the first message persists) doesn't drop this freshly-opened, selected
    // conversation — loadConversationsFromStore only preserves DB-absent
    // conversations that are in guards.pendingConversations.
    guards.pendingConversations.add(convId);
    setStore("conversations", (prev) => [...prev, conv]);
    return conv;
  }

  function findOrCreateGroupConversation(
    groupId: string,
    title: string,
    accountId: string
  ): Conversation {
    const convId = `group-${groupId}`;
    const existing = store.conversations.find((c) => c.id === convId);
    if (existing) return existing;
    const conv: Conversation = {
      id: convId,
      title,
      accountId,
      serverUrl: getServerUrl(accountId),
      groupId,
      isGroup: true,
      isRequest: false,
      isBlocked: false,
      lastMessageKind: 0,
    };
    // See findOrCreateDMConversation — a just-created group isn't in the DB
    // summaries yet, so preserve it across reloads until its state syncs.
    guards.pendingConversations.add(convId);
    setStore("conversations", (prev) => [...prev, conv]);
    return conv;
  }

  function displayName(did: string, accountId: string): string {
    const own = store.accounts.find((a) => a.id === did);
    if (own) return own.displayName;
    // Reactive read: Solid tracks this access so components re-render when
    // the cache is populated by the async fetch below.
    const cached = displayNameCache[did];
    if (cached !== undefined) return cached;
    // Guard against duplicate in-flight fetches for the same DID.
    if (!displayNamePending.has(did)) {
      displayNamePending.add(did);
      void serviceFor(accountId)
        .contactDisplayName(did)
        .then((name) => {
          // Always cache — even empty strings — to prevent infinite refetch.
          // Only update conversation titles when a non-empty name arrives.
          setDisplayNameCache(did, name);
          if (name) {
            store.conversations.forEach((c, i) => {
              if (c.recipientDid === did && c.title === did) {
                setStore("conversations", i, "title", name);
              }
            });
          }
        })
        .catch((e: unknown) => {
          console.warn("contactDisplayName failed:", did, e);
        })
        .finally(() => {
          displayNamePending.delete(did);
        });
    }
    return did;
  }

  // Reactive is-bot lookup (docs/54 bot presentation). Returns the cached value
  // (default false) and fires a getAccountInfo fetch to populate it; the read is
  // tracked by Solid, so a bot avatar re-renders as a hexagon once resolved. Own
  // accounts are never bots. Mirrors the displayName cache pattern.
  function isBot(did: string, accountId: string): boolean {
    if (store.accounts.some((a) => a.id === did)) return false;
    const cached = isBotCache[did];
    if (cached !== undefined) return cached;
    if (!isBotPending.has(did)) {
      isBotPending.add(did);
      void serviceFor(accountId)
        .getAccountInfo(did)
        .then((info) => setIsBotCache(did, info.isBot))
        .catch((e: unknown) => {
          console.warn("getAccountInfo (isBot) failed:", did, e);
        })
        .finally(() => {
          isBotPending.delete(did);
        });
    }
    return false;
  }

  // Non-fetching cache read (notification titles) — returns undefined on a miss
  // instead of firing the async resolve like `displayName` does.
  function cachedDisplayName(did: string): string | undefined {
    return displayNameCache[did];
  }

  // Reset the reactive display-name / is-bot caches so components get a
  // reactive update on logout/mode-switch. Called from resetSession.
  function resetCaches() {
    setDisplayNameCache(reconcile({}));
    displayNamePending.clear();
    setIsBotCache(reconcile({}));
    isBotPending.clear();
  }

  // ── Deep links (T61) ────────────────────────────────────────────────────────

  // True if `raw` is a routable deep link (an avalanche:// or
  // go.theavalanche.net URL with a recognized action) — lets the recipient
  // field distinguish a pasted contact/invite link from a bare DID.
  function isDeepLink(raw: string): boolean {
    return parseDeepLink(raw) !== null;
  }

  // Route a deep link like iOS AppState.handleDeepLink: open a DM for
  // conversation/<did>, and for i/<token> jump straight to the inviter's DM if
  // already on that server, else hand the token to onboarding.
  function handleDeepLink(raw: string) {
    const parsed = parseDeepLink(raw);
    if (!parsed) return;
    const { action, arg } = parsed;

    if (action === "conversation") {
      // No conversation context in a bare contact link, so default to the first
      // account (iOS does the same for ambiguous deep links — AppState.swift).
      const accountId = store.accounts[0]?.id ?? null;
      if (!arg || accountId === null) return;
      const conv = findOrCreateDMConversation(arg, accountId);
      setStore("selectedTab", "chats");
      setSelectedConversationId(conv.id);
      return;
    }

    if (action === "i" || action === "invite") {
      const token = arg;
      const decoded = decodeInviteToken(token);
      if (decoded) {
        const account = store.accounts.find((a) =>
          a.servers.some((s) => trimSlashes(s.url) === trimSlashes(decoded.serverUrl))
        );
        if (account && decoded.inviterDid) {
          // Already on this server — open the inviter's DM directly.
          const conv = findOrCreateDMConversation(decoded.inviterDid, account.id);
          setStore("selectedTab", "chats");
          setSelectedConversationId(conv.id);
          return;
        }
      }
      // Not on the server (or undecodable) → start onboarding with the token.
      setStore("pendingInviteToken", token);
    }
  }

  return {
    loadConversationsFromStore,
    reloadConversations,
    findOrCreateDMConversation,
    displayName,
    isBot,
    isDeepLink,
    handleDeepLink,
    accountIdForConversation,
    getServerUrl,
    findOrCreateGroupConversation,
    cachedDisplayName,
    resetCaches,
  };
}
