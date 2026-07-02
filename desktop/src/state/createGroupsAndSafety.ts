import type { SetStoreFunction } from "solid-js/store";
import type { Conversation } from "../models";
import type {
  JoinResultFfi,
  ContactRowFfi,
} from "../services/AvalancheService";
import type { Services } from "./createServices";
import type { AppContextValue, AppStore, SessionGuards } from "./types";

export interface GroupsAndSafetyDeps {
  store: AppStore;
  setStore: SetStoreFunction<AppStore>;
  serviceFor: Services["serviceFor"];
  guards: SessionGuards;
  reloadConversations: () => Promise<void>;
  findOrCreateGroupConversation: (
    groupId: string,
    title: string,
    accountId: string
  ) => Conversation;
  selectedConversationId: () => string | null;
  setSelectedConversationId: (id: string | null) => void;
}

// Track B (group create/join/leave) and Track D (message requests, blocking,
// disappearing-message timers) — thin service calls plus conversation-list
// refreshes. Pick-typed — see the note in createConversations.ts.
export type GroupsAndSafety = Pick<
  AppContextValue,
  | "createGroupAndOpen"
  | "joinViaLink"
  | "leaveGroup"
  | "acceptRequest"
  | "deleteRequest"
  | "reportAndBlock"
  | "blockContact"
  | "unblockContact"
  | "listBlocked"
  | "getConversationTimer"
  | "setConversationTimer"
>;

export function createGroupsAndSafety(deps: GroupsAndSafetyDeps): GroupsAndSafety {
  const {
    store,
    setStore,
    serviceFor,
    guards,
    reloadConversations,
    findOrCreateGroupConversation,
    selectedConversationId,
    setSelectedConversationId,
  } = deps;

  // ── Track B: groups + join via link ────────────────────────────────────────

  async function createGroupAndOpen(
    accountId: string,
    title: string,
    recipientDids: string[],
    expirySeconds: number
  ): Promise<Conversation> {
    const svc = serviceFor(accountId);
    const created = await svc.createGroup(title, "", expirySeconds);
    const groupId = created.groupId;
    // Best-effort fan-out: one failed invite must not abort the rest.
    for (const did of recipientDids) {
      try {
        await svc.inviteMember(groupId, did, 0);
      } catch (e) {
        console.warn("inviteMember failed for", did, e);
      }
    }
    const conv = findOrCreateGroupConversation(groupId, title, accountId);
    return conv;
  }

  // The pasted-/opened-link entry (NewConversationView → handleDeepLink) covers
  // contact and server-invite links (conversation/<did>, i/<token>). A *group*
  // join link — which would carry the master key + invite-link password
  // (docs/03 §3.10) into this handler — has no defined URL wire format in iOS or
  // app-core yet, so there is deliberately no UI that builds those args here.
  // Wire it once that format exists; the handler itself is complete.
  async function joinViaLink(
    accountId: string,
    masterKey: number[],
    hostingServerUrl: string,
    password: number[]
  ): Promise<JoinResultFfi> {
    const result = await serviceFor(accountId).joinViaLink(masterKey, hostingServerUrl, password);
    await reloadConversations();
    return result;
  }

  async function leaveGroup(conversation: Conversation) {
    if (!conversation.groupId) return;
    try {
      await serviceFor(conversation.accountId).leaveGroup(conversation.groupId);
    } catch (e) {
      // Don't flip the UI to the irreversible read-only "you left" state when
      // the server-side leave didn't actually happen — the user is still a
      // member and can keep participating.
      console.warn("leaveGroup failed:", e);
      throw e;
    }
    // Keep the conversation visible but read-only (Signal-style): mark it left
    // and track it as pending so the next loadConversationsFromStore preserves
    // it if app-core stops returning the left group.
    const idx = store.conversations.findIndex((c) => c.id === conversation.id);
    if (idx >= 0) setStore("conversations", idx, "hasLeft", true);
    guards.pendingConversations.add(conversation.id);
  }

  // ── Track D: message requests / blocking / timers ──────────────────────────

  async function acceptRequest(conversation: Conversation) {
    if (!conversation.recipientDid) return;
    await serviceFor(conversation.accountId)
      .acceptRequest(conversation.recipientDid)
      .catch((e: unknown) => {
        console.warn("acceptRequest failed:", e);
      });
    await reloadConversations();
  }

  async function deleteRequest(conversation: Conversation) {
    if (!conversation.recipientDid) return;
    await serviceFor(conversation.accountId)
      .deleteRequest(conversation.recipientDid)
      .catch((e: unknown) => {
        console.warn("deleteRequest failed:", e);
      });
    if (selectedConversationId() === conversation.id) setSelectedConversationId(null);
    await reloadConversations();
  }

  async function reportAndBlock(conversation: Conversation, reason: string) {
    if (!conversation.recipientDid) return;
    await serviceFor(conversation.accountId)
      .reportAndBlock(conversation.recipientDid, reason)
      .catch((e: unknown) => {
        console.warn("reportAndBlock failed:", e);
      });
    await reloadConversations();
  }

  async function blockContact(accountId: string, did: string) {
    await serviceFor(accountId).blockContact(did).catch((e: unknown) => {
      console.warn("blockContact failed:", e);
    });
    await reloadConversations();
  }

  async function unblockContact(accountId: string, did: string) {
    await serviceFor(accountId).unblockContact(did).catch((e: unknown) => {
      console.warn("unblockContact failed:", e);
    });
    await reloadConversations();
  }

  // Blocked contacts are per-account; aggregate across every signed-in account so
  // the settings list shows them all, each row tagged with its owning accountId.
  async function listBlocked(): Promise<Array<ContactRowFfi & { accountId: string }>> {
    // Per-account lists are independent — fetch them concurrently, then flatten.
    const perAccount = await Promise.all(
      store.accounts.map(async (account) => {
        const accountRows = await serviceFor(account.id)
          .listBlocked()
          .catch((e: unknown) => {
            console.warn("listBlocked failed:", e);
            return [] as ContactRowFfi[];
          });
        return accountRows.map((r) => ({ ...r, accountId: account.id }));
      })
    );
    return perAccount.flat();
  }

  function getConversationTimer(
    accountId: string,
    conversationId: string
  ): Promise<number | null> {
    return serviceFor(accountId).getConversationTimer(conversationId).catch((e: unknown) => {
      console.warn("getConversationTimer failed:", e);
      return null;
    });
  }

  async function setConversationTimer(
    accountId: string,
    recipientDid: string,
    expirySecs: number | null
  ) {
    await serviceFor(accountId).setConversationTimer(recipientDid, expirySecs).catch((e: unknown) => {
      console.warn("setConversationTimer failed:", e);
    });
  }

  return {
    createGroupAndOpen,
    joinViaLink,
    leaveGroup,
    acceptRequest,
    deleteRequest,
    reportAndBlock,
    blockContact,
    unblockContact,
    listBlocked,
    getConversationTimer,
    setConversationTimer,
  };
}
