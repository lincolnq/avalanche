import { commands } from "../bindings";
import { invoke } from "@tauri-apps/api/core";
import type { AvalancheService } from "./AvalancheService";
import type {
  AccountResult,
  ConnectionState,
  CreatedGroupFfi,
  GroupSummaryFfi,
  IncomingEvent,
} from "../bindings";

// Unwrap the typedError wrapper from specta-generated command functions.
async function ok<T>(result: Promise<{ status: "ok"; data: T } | { status: "error"; error: string }>): Promise<T> {
  const r = await result;
  if (r.status === "error") throw new Error(r.error);
  return r.data;
}

export class DevServerAvalancheService implements AvalancheService {
  // ── Account factory ────────────────────────────────────────────────

  async createAccount(
    serverUrl: string,
    dbPath: string,
    dbKey: string,
    prfOutput: number[],
    displayName: string,
    inviteToken: string | null,
  ): Promise<AccountResult> {
    return ok(commands.createAccount(serverUrl, dbPath, dbKey, prfOutput, displayName, inviteToken));
  }

  async login(dbPath: string, dbKey: string): Promise<AccountResult> {
    return ok(commands.login(dbPath, dbKey));
  }

  async recoverFromBlob(
    serverUrl: string,
    did: string,
    prfOutput: number[],
    dbPath: string,
    dbKey: string,
    displayName: string,
  ): Promise<AccountResult> {
    return ok(commands.recoverFromBlob(serverUrl, did, prfOutput, dbPath, dbKey, displayName));
  }

  async recoverFromPhrase(
    phrase: string,
    serverUrl: string,
    did: string,
    dbPath: string,
    dbKey: string,
    displayName: string,
  ): Promise<AccountResult> {
    return ok(commands.recoverFromPhrase(phrase, serverUrl, did, dbPath, dbKey, displayName));
  }

  // ── Core messaging ─────────────────────────────────────────────────

  async sendDm(recipientDid: string, plaintext: number[], sentAtMs: number): Promise<void> {
    await ok(commands.sendDm(recipientDid, plaintext, sentAtMs));
  }

  async sendGroupMessage(groupId: string, plaintext: number[], sentAtMs: number): Promise<void> {
    await ok(commands.sendGroupMessage(groupId, plaintext, sentAtMs));
  }

  async nextEvents(): Promise<IncomingEvent[]> {
    return ok(commands.nextEvents());
  }

  async saveMessage(msg: import("../bindings").StoredMessageFfi): Promise<void> {
    await ok(commands.saveMessage(msg));
  }

  async loadConversations(): Promise<import("../bindings").ConversationSummaryFfi[]> {
    return ok(commands.loadConversations());
  }

  async loadMessages(conversationId: string): Promise<import("../bindings").StoredMessageFfi[]> {
    return ok(commands.loadMessages(conversationId));
  }

  async markMessagesRead(conversationId: string, upToSentAtMs: number): Promise<number> {
    return ok(commands.markMessagesRead(conversationId, upToSentAtMs));
  }

  async unreadCount(conversationId: string): Promise<number> {
    return ok(commands.unreadCount(conversationId));
  }

  async receiveMessages(): Promise<import("../bindings").DecryptedMessage[]> {
    return ok(commands.receiveMessages());
  }

  async sendReadReceipt(recipientDid: string, timestamps: number[]): Promise<void> {
    await ok(commands.sendReadReceipt(recipientDid, timestamps));
  }

  // ── Identity / contacts ────────────────────────────────────────────

  async did(): Promise<string> {
    return ok(commands.did());
  }

  async deviceId(): Promise<number> {
    return ok(commands.deviceId());
  }

  async ownDisplayName(): Promise<string> {
    return ok(commands.ownDisplayName());
  }

  async setDisplayName(displayName: string): Promise<void> {
    await ok(commands.setDisplayName(displayName));
  }

  async hasRecovery(): Promise<boolean> {
    return ok(commands.hasRecovery());
  }

  async contactDisplayName(did: string): Promise<string> {
    return ok(commands.contactDisplayName(did));
  }

  async getAccountInfo(did: string): Promise<import("../bindings").AccountInfoFfi> {
    return ok(commands.getAccountInfo(did));
  }

  async refreshContactProfile(did: string): Promise<boolean> {
    return ok(commands.refreshContactProfile(did));
  }

  async listContacts(): Promise<import("../bindings").ContactRowFfi[]> {
    return ok(commands.listContacts());
  }

  async touchContact(did: string, curated: boolean): Promise<void> {
    await ok(commands.touchContact(did, curated));
  }

  async fetchAndCacheProfile(did: string, profileKey: Uint8Array): Promise<void> {
    await ok(commands.fetchAndCacheProfile(did, Array.from(profileKey)));
  }

  async primeContactProfile(did: string, displayName: string, profileKey: Uint8Array): Promise<void> {
    await ok(commands.primeContactProfile(did, displayName, Array.from(profileKey)));
  }

  async blockContact(did: string): Promise<void> {
    await ok(commands.blockContact(did));
  }

  async unblockContact(did: string): Promise<void> {
    await ok(commands.unblockContact(did));
  }

  // ── Message requests / safety ──────────────────────────────────────

  async acceptRequest(did: string): Promise<void> {
    await ok(commands.acceptRequest(did));
  }

  async deleteRequest(did: string): Promise<void> {
    await ok(commands.deleteRequest(did));
  }

  async setPendingRequest(did: string, pending: boolean): Promise<void> {
    await ok(commands.setPendingRequest(did, pending));
  }

  async reportAndBlock(did: string, reason: string): Promise<void> {
    await ok(commands.reportAndBlock(did, reason));
  }

  async listBlocked(): Promise<import("../bindings").ContactRowFfi[]> {
    return ok(commands.listBlocked());
  }

  // ── Disappearing-message timers ────────────────────────────────────

  async getConversationTimer(conversationId: string): Promise<number | null> {
    return ok(commands.getConversationTimer(conversationId));
  }

  async setConversationTimer(recipientDid: string, expirySecs: number | null): Promise<void> {
    await ok(commands.setConversationTimer(recipientDid, expirySecs));
  }

  async deleteExpiredMessages(): Promise<string[]> {
    return ok(commands.deleteExpiredMessages());
  }

  // ── Account lifecycle ──────────────────────────────────────────────

  async leaveServer(): Promise<void> {
    await ok(commands.leaveServer());
  }

  async deleteIdentity(): Promise<void> {
    await ok(commands.deleteIdentity());
  }

  // ── Session management ──────────────────────────────────────────────

  async clearSession(): Promise<void> {
    await invoke("clear_session");
  }

  // ── Projects ───────────────────────────────────────────────────────

  async fetchProjects(): Promise<import("../bindings").ProjectInfoFfi[]> {
    return ok(commands.fetchProjects());
  }

  async requestProjectToken(projectUrl: string): Promise<string> {
    return ok(commands.requestProjectToken(projectUrl));
  }

  async validateInvite(token: string): Promise<import("../bindings").InviteInfo> {
    return ok(commands.validateInvite(token));
  }

  // ── Connection state ───────────────────────────────────────────────

  async connectionState(): Promise<ConnectionState> {
    return ok(commands.connectionState());
  }

  async waitForConnectionStateChange(last: ConnectionState): Promise<ConnectionState> {
    return ok(commands.waitForConnectionStateChange(last));
  }

  // ── Groups ─────────────────────────────────────────────────────────

  async createGroup(title: string, description: string, expirySeconds: number): Promise<CreatedGroupFfi> {
    return ok(commands.createGroup(title, description, expirySeconds));
  }

  async fetchGroupState(groupId: string): Promise<GroupSummaryFfi> {
    return ok(commands.fetchGroupState(groupId));
  }

  async cachedGroupState(groupId: string): Promise<GroupSummaryFfi | null> {
    return ok(commands.cachedGroupState(groupId));
  }

  async inviteMember(groupId: string, recipientDid: string, role: number): Promise<void> {
    await ok(commands.inviteMember(groupId, recipientDid, role));
  }

  async acceptInvite(groupId: string): Promise<void> {
    await ok(commands.acceptInvite(groupId));
  }

  async declineInvite(groupId: string): Promise<void> {
    await ok(commands.declineInvite(groupId));
  }

  async cancelJoinRequest(groupId: string): Promise<void> {
    await ok(commands.cancelJoinRequest(groupId));
  }

  async approveJoinRequest(groupId: string, encryptedMemberId: string): Promise<void> {
    await ok(commands.approveJoinRequest(groupId, encryptedMemberId));
  }

  async denyJoinRequest(groupId: string, encryptedMemberId: string): Promise<void> {
    await ok(commands.denyJoinRequest(groupId, encryptedMemberId));
  }

  async removeMember(groupId: string, encryptedMemberId: string): Promise<void> {
    await ok(commands.removeMember(groupId, encryptedMemberId));
  }

  async leaveGroup(groupId: string): Promise<void> {
    await ok(commands.leaveGroup(groupId));
  }

  async isGroupMember(groupId: string): Promise<boolean> {
    return ok(commands.isGroupMember(groupId));
  }

  async changeMemberRole(groupId: string, encryptedMemberId: string, newRole: number): Promise<void> {
    await ok(commands.changeMemberRole(groupId, encryptedMemberId, newRole));
  }

  async setGroupExpiry(groupId: string, expirySeconds: number): Promise<void> {
    await ok(commands.setGroupExpiry(groupId, expirySeconds));
  }

  async setGroupTitle(groupId: string, newTitle: string): Promise<void> {
    await ok(commands.setGroupTitle(groupId, newTitle));
  }

  async groupExpirySeconds(groupId: string): Promise<number> {
    return ok(commands.groupExpirySeconds(groupId));
  }

  async applyPendingGroupChanges(groupId: string): Promise<number> {
    return ok(commands.applyPendingGroupChanges(groupId));
  }

  async listGroups(): Promise<string[]> {
    return ok(commands.listGroups());
  }

  async joinViaLink(
    masterKey: number[],
    hostingServerUrl: string,
    password: number[],
  ): Promise<import("../bindings").JoinResultFfi> {
    return ok(commands.joinViaLink(masterKey, hostingServerUrl, password));
  }

  // ── Reactions / edit / delete ──────────────────────────────────────

  async sendReaction(
    target: import("../bindings").MessageTarget,
    targetAuthor: string,
    targetSentAtMs: number,
    emoji: string,
    remove: boolean,
    sentAtMs: number,
  ): Promise<void> {
    await ok(commands.sendReaction(target, targetAuthor, targetSentAtMs, emoji, remove, sentAtMs));
  }

  async sendEdit(
    target: import("../bindings").MessageTarget,
    targetSentAtMs: number,
    newBody: string,
    sentAtMs: number,
  ): Promise<void> {
    await ok(commands.sendEdit(target, targetSentAtMs, newBody, sentAtMs));
  }

  async sendDelete(
    target: import("../bindings").MessageTarget,
    targetAuthor: string,
    targetSentAtMs: number,
    forEveryone: boolean,
    sentAtMs: number,
  ): Promise<void> {
    await ok(commands.sendDelete(target, targetAuthor, targetSentAtMs, forEveryone, sentAtMs));
  }

  async loadReactions(conversationId: string): Promise<import("../bindings").ReactionFfi[]> {
    return ok(commands.loadReactions(conversationId));
  }

  async loadMessageRevisions(conversationId: string, author: string, sentAtMs: number): Promise<import("../bindings").MessageRevisionFfi[]> {
    return ok(commands.loadMessageRevisions(conversationId, author, sentAtMs));
  }
}
