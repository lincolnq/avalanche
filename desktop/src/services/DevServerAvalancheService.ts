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
  // Bound at construction by AppContext's serviceFor(accountId): every
  // per-account command passes this as its first arg so the Rust backend
  // resolves the right AppCore from its `cores` map. Account-less factory/
  // pure commands (createAccount, validateInvite, ...) ignore it.
  constructor(private readonly accountId: string = "") {}

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

  // ── Device linking (T71) ───────────────────────────────────────────

  async deviceLinkCreatePairing(mailboxServer: string | null): Promise<string> {
    return ok(commands.deviceLinkCreatePairing(mailboxServer));
  }

  async deviceLinkAcceptPairing(code: string): Promise<void> {
    await ok(commands.deviceLinkAcceptPairing(code));
  }

  async deviceLinkAwaitStep(dbPath: string, dbKey: string): Promise<AccountResult | null> {
    return ok(commands.deviceLinkAwaitStep(dbPath, dbKey));
  }

  async deviceLinkReset(): Promise<void> {
    await ok(commands.deviceLinkReset());
  }

  async linkCreatePairing(mailboxServer: string | null): Promise<string> {
    return ok(commands.linkCreatePairing(this.accountId, mailboxServer));
  }

  async linkAcceptPairing(code: string): Promise<void> {
    await ok(commands.linkAcceptPairing(this.accountId, code));
  }

  async linkSendBundleStep(): Promise<boolean> {
    return ok(commands.linkSendBundleStep(this.accountId));
  }

  // ── Core messaging ─────────────────────────────────────────────────

  async sendDm(recipientDid: string, plaintext: number[], sentAtMs: number): Promise<void> {
    await ok(commands.sendDm(this.accountId, recipientDid, plaintext, sentAtMs));
  }

  async sendGroupMessage(groupId: string, plaintext: number[], sentAtMs: number): Promise<void> {
    await ok(commands.sendGroupMessage(this.accountId, groupId, plaintext, sentAtMs));
  }

  async nextEvents(): Promise<IncomingEvent[]> {
    return ok(commands.nextEvents(this.accountId));
  }

  async saveMessage(msg: import("../bindings").StoredMessageFfi): Promise<void> {
    await ok(commands.saveMessage(this.accountId, msg));
  }

  async loadConversations(): Promise<import("../bindings").ConversationSummaryFfi[]> {
    return ok(commands.loadConversations(this.accountId));
  }

  async loadMessages(conversationId: string): Promise<import("../bindings").StoredMessageFfi[]> {
    return ok(commands.loadMessages(this.accountId, conversationId));
  }

  async markMessagesRead(conversationId: string, upToSentAtMs: number): Promise<number> {
    return ok(commands.markMessagesRead(this.accountId, conversationId, upToSentAtMs));
  }

  async unreadCount(conversationId: string): Promise<number> {
    return ok(commands.unreadCount(this.accountId, conversationId));
  }

  async receiveMessages(): Promise<import("../bindings").DecryptedMessage[]> {
    return ok(commands.receiveMessages(this.accountId));
  }

  async sendReadReceipt(recipientDid: string, timestamps: number[]): Promise<void> {
    await ok(commands.sendReadReceipt(this.accountId, recipientDid, timestamps));
  }

  // ── Identity / contacts ────────────────────────────────────────────

  async did(): Promise<string> {
    return ok(commands.did(this.accountId));
  }

  async deviceId(): Promise<number> {
    return ok(commands.deviceId(this.accountId));
  }

  async ownDisplayName(): Promise<string> {
    return ok(commands.ownDisplayName(this.accountId));
  }

  async setDisplayName(displayName: string): Promise<void> {
    await ok(commands.setDisplayName(this.accountId, displayName));
  }

  async hasRecovery(): Promise<boolean> {
    return ok(commands.hasRecovery(this.accountId));
  }

  async updateRecoveryBlob(prfOutput: number[], servers: string[]): Promise<void> {
    await ok(commands.updateRecoveryBlob(this.accountId, prfOutput, servers));
  }

  async homeServer(): Promise<string> {
    return ok(commands.homeServer(this.accountId));
  }

  async generateRecoveryPhrase(): Promise<string> {
    return ok(commands.generateRecoveryPhrase());
  }

  async recoveryPhraseToSeed(phrase: string): Promise<number[]> {
    return ok(commands.recoveryPhraseToSeed(phrase));
  }

  async deriveDidFromPasskey(prfOutput: number[], signupServerUrl: string): Promise<string> {
    return ok(commands.deriveDidFromPasskey(prfOutput, signupServerUrl));
  }

  async contactDisplayName(did: string): Promise<string> {
    return ok(commands.contactDisplayName(this.accountId, did));
  }

  async cachedDisplayNames(dids: string[]): Promise<Record<string, string>> {
    return ok(commands.cachedDisplayNames(this.accountId, dids));
  }

  async getAccountInfo(did: string): Promise<import("../bindings").AccountInfoFfi> {
    return ok(commands.getAccountInfo(this.accountId, did));
  }

  async refreshContactProfile(did: string): Promise<boolean> {
    return ok(commands.refreshContactProfile(this.accountId, did));
  }

  async listContacts(): Promise<import("../bindings").ContactRowFfi[]> {
    return ok(commands.listContacts(this.accountId));
  }

  async touchContact(did: string, curated: boolean): Promise<void> {
    await ok(commands.touchContact(this.accountId, did, curated));
  }

  async fetchAndCacheProfile(did: string, profileKey: Uint8Array): Promise<void> {
    await ok(commands.fetchAndCacheProfile(this.accountId, did, Array.from(profileKey)));
  }

  async primeContactProfile(did: string, displayName: string, profileKey: Uint8Array): Promise<void> {
    await ok(commands.primeContactProfile(this.accountId, did, displayName, Array.from(profileKey)));
  }

  async blockContact(did: string): Promise<void> {
    await ok(commands.blockContact(this.accountId, did));
  }

  async unblockContact(did: string): Promise<void> {
    await ok(commands.unblockContact(this.accountId, did));
  }

  // ── Message requests / safety ──────────────────────────────────────

  async acceptRequest(did: string): Promise<void> {
    await ok(commands.acceptRequest(this.accountId, did));
  }

  async deleteRequest(did: string): Promise<void> {
    await ok(commands.deleteRequest(this.accountId, did));
  }

  async setPendingRequest(did: string, pending: boolean): Promise<void> {
    await ok(commands.setPendingRequest(this.accountId, did, pending));
  }

  async reportAndBlock(did: string, reason: string): Promise<void> {
    await ok(commands.reportAndBlock(this.accountId, did, reason));
  }

  async listBlocked(): Promise<import("../bindings").ContactRowFfi[]> {
    return ok(commands.listBlocked(this.accountId));
  }

  // ── Disappearing-message timers ────────────────────────────────────

  async getConversationTimer(conversationId: string): Promise<number | null> {
    return ok(commands.getConversationTimer(this.accountId, conversationId));
  }

  async setConversationTimer(recipientDid: string, expirySecs: number | null): Promise<void> {
    await ok(commands.setConversationTimer(this.accountId, recipientDid, expirySecs));
  }

  async deleteExpiredMessages(): Promise<string[]> {
    return ok(commands.deleteExpiredMessages(this.accountId));
  }

  // ── Account lifecycle ──────────────────────────────────────────────

  async leaveServer(): Promise<void> {
    await ok(commands.leaveServer(this.accountId));
  }

  async deleteIdentity(): Promise<void> {
    await ok(commands.deleteIdentity(this.accountId));
  }

  // ── Session management ──────────────────────────────────────────────

  async clearSession(): Promise<void> {
    await invoke("clear_session", { accountId: this.accountId });
  }

  // ── Projects ───────────────────────────────────────────────────────

  async fetchProjects(): Promise<import("../bindings").ProjectInfoFfi[]> {
    return ok(commands.fetchProjects(this.accountId));
  }

  async requestProjectToken(projectUrl: string): Promise<string> {
    return ok(commands.requestProjectToken(this.accountId, projectUrl));
  }

  async validateInvite(token: string): Promise<import("../bindings").InviteInfo> {
    return ok(commands.validateInvite(token));
  }

  // ── Connection state ───────────────────────────────────────────────

  async connectionState(): Promise<ConnectionState> {
    return ok(commands.connectionState(this.accountId));
  }

  async waitForConnectionStateChange(last: ConnectionState): Promise<ConnectionState> {
    return ok(commands.waitForConnectionStateChange(this.accountId, last));
  }

  async setAppActive(active: boolean): Promise<void> {
    await ok(commands.setAppActive(this.accountId, active));
  }

  async reconnectNow(): Promise<void> {
    await ok(commands.reconnectNow(this.accountId));
  }

  // ── Groups ─────────────────────────────────────────────────────────

  async createGroup(title: string, description: string, expirySeconds: number): Promise<CreatedGroupFfi> {
    return ok(commands.createGroup(this.accountId, title, description, expirySeconds));
  }

  async fetchGroupState(groupId: string): Promise<GroupSummaryFfi> {
    return ok(commands.fetchGroupState(this.accountId, groupId));
  }

  async cachedGroupState(groupId: string): Promise<GroupSummaryFfi | null> {
    return ok(commands.cachedGroupState(this.accountId, groupId));
  }

  async inviteMember(groupId: string, recipientDid: string, role: number): Promise<void> {
    await ok(commands.inviteMember(this.accountId, groupId, recipientDid, role));
  }

  async acceptInvite(groupId: string): Promise<void> {
    await ok(commands.acceptInvite(this.accountId, groupId));
  }

  async declineInvite(groupId: string): Promise<void> {
    await ok(commands.declineInvite(this.accountId, groupId));
  }

  async cancelJoinRequest(groupId: string): Promise<void> {
    await ok(commands.cancelJoinRequest(this.accountId, groupId));
  }

  async approveJoinRequest(groupId: string, encryptedMemberId: string): Promise<void> {
    await ok(commands.approveJoinRequest(this.accountId, groupId, encryptedMemberId));
  }

  async denyJoinRequest(groupId: string, encryptedMemberId: string): Promise<void> {
    await ok(commands.denyJoinRequest(this.accountId, groupId, encryptedMemberId));
  }

  async removeMember(groupId: string, encryptedMemberId: string): Promise<void> {
    await ok(commands.removeMember(this.accountId, groupId, encryptedMemberId));
  }

  async leaveGroup(groupId: string): Promise<void> {
    await ok(commands.leaveGroup(this.accountId, groupId));
  }

  async isGroupMember(groupId: string): Promise<boolean> {
    return ok(commands.isGroupMember(this.accountId, groupId));
  }

  async changeMemberRole(groupId: string, encryptedMemberId: string, newRole: number): Promise<void> {
    await ok(commands.changeMemberRole(this.accountId, groupId, encryptedMemberId, newRole));
  }

  async setGroupExpiry(groupId: string, expirySeconds: number): Promise<void> {
    await ok(commands.setGroupExpiry(this.accountId, groupId, expirySeconds));
  }

  async setGroupTitle(groupId: string, newTitle: string): Promise<void> {
    await ok(commands.setGroupTitle(this.accountId, groupId, newTitle));
  }

  async groupExpirySeconds(groupId: string): Promise<number> {
    return ok(commands.groupExpirySeconds(this.accountId, groupId));
  }

  async applyPendingGroupChanges(groupId: string): Promise<number> {
    return ok(commands.applyPendingGroupChanges(this.accountId, groupId));
  }

  async listGroups(): Promise<string[]> {
    return ok(commands.listGroups(this.accountId));
  }

  async joinViaLink(
    masterKey: number[],
    hostingServerUrl: string,
    password: number[],
  ): Promise<import("../bindings").JoinResultFfi> {
    return ok(commands.joinViaLink(this.accountId, masterKey, hostingServerUrl, password));
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
    await ok(commands.sendReaction(this.accountId, target, targetAuthor, targetSentAtMs, emoji, remove, sentAtMs));
  }

  async sendEdit(
    target: import("../bindings").MessageTarget,
    targetSentAtMs: number,
    newBody: string,
    sentAtMs: number,
  ): Promise<void> {
    await ok(commands.sendEdit(this.accountId, target, targetSentAtMs, newBody, sentAtMs));
  }

  async sendDelete(
    target: import("../bindings").MessageTarget,
    targetAuthor: string,
    targetSentAtMs: number,
    forEveryone: boolean,
    sentAtMs: number,
  ): Promise<void> {
    await ok(commands.sendDelete(this.accountId, target, targetAuthor, targetSentAtMs, forEveryone, sentAtMs));
  }

  async loadReactions(conversationId: string): Promise<import("../bindings").ReactionFfi[]> {
    return ok(commands.loadReactions(this.accountId, conversationId));
  }

  async loadMessageRevisions(conversationId: string, author: string, sentAtMs: number): Promise<import("../bindings").MessageRevisionFfi[]> {
    return ok(commands.loadMessageRevisions(this.accountId, conversationId, author, sentAtMs));
  }

  // ── Attachments / link previews / external links ───────────────────

  async uploadAttachment(
    plaintext: number[],
    contentType: string,
    fileName: string | null,
    width: number,
    height: number,
    durationMs: number,
    thumbnail: number[],
    flags: number,
  ): Promise<import("../bindings").AttachmentFfi> {
    return ok(
      commands.uploadAttachment(this.accountId, plaintext, contentType, fileName, width, height, durationMs, thumbnail, flags),
    );
  }

  async downloadAttachment(attachment: import("../bindings").AttachmentFfi): Promise<number[]> {
    return ok(commands.downloadAttachment(this.accountId, attachment));
  }

  async sendMessageWithAttachments(
    target: import("../bindings").MessageTarget,
    body: string,
    attachments: import("../bindings").AttachmentFfi[],
    previews: import("../bindings").LinkPreviewFfi[],
    sentAtMs: number,
  ): Promise<void> {
    await ok(commands.sendMessageWithAttachments(this.accountId, target, body, attachments, previews, sentAtMs));
  }

  async openExternal(url: string): Promise<void> {
    await ok(commands.openExternal(url));
  }

  async fetchLinkPreview(url: string): Promise<import("../bindings").LinkPreviewMetaFfi> {
    return ok(commands.fetchLinkPreview(url));
  }
}
