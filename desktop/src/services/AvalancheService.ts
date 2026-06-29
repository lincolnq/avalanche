// Service layer — Desktop parity of iOS AppCoreProtocol.
// Types are code-generated from Rust via tauri-specta → ../bindings.ts.
// The AvalancheService interface is the parity abstraction; MockAvalancheService
// and DevServerAvalancheService both implement it.

export enum ServiceMode {
  Mock = "mock",
  DevServer = "devServer",
}

// ── FFI types — re-exported from the generated bindings ──────────────────────

export type {
  AccountInfoFfi,
  AccountResult,
  AttachmentFfi,
  ConnectionState,
  ContactRowFfi,
  ConversationSummaryFfi,
  CreatedGroupFfi,
  DecryptedMessage,
  DeliveryStatusUpdate,
  GroupEventKind,
  GroupMemberFfi,
  GroupMetadataEvent,
  GroupPendingFfi,
  GroupSummaryFfi,
  IncomingEvent,
  InviteInfo,
  JoinResultFfi,
  LinkPreviewFfi,
  LinkPreviewMetaFfi,
  MessageRevisionFfi,
  MessageTarget,
  ProjectInfoFfi,
  ReactionFfi,
  StoredMessageFfi,
} from "../bindings";

// ── Service interface ─────────────────────────────────────────────────────────

export interface AvalancheService {
  // Account factory
  createAccount(
    serverUrl: string,
    dbPath: string,
    dbKey: string,
    prfOutput: number[],
    displayName: string,
    inviteToken: string | null
  ): Promise<import("../bindings").AccountResult>;
  login(dbPath: string, dbKey: string): Promise<import("../bindings").AccountResult>;
  recoverFromBlob(
    serverUrl: string,
    did: string,
    prfOutput: number[],
    dbPath: string,
    dbKey: string,
    displayName: string
  ): Promise<import("../bindings").AccountResult>;
  recoverFromPhrase(
    phrase: string,
    serverUrl: string,
    did: string,
    dbPath: string,
    dbKey: string,
    displayName: string
  ): Promise<import("../bindings").AccountResult>;

  // Device linking (T71). New-device side (account-less) drives
  // create/accept → poll deviceLinkAwaitStep until it returns the account;
  // existing-device side drives create/accept → poll linkSendBundleStep.
  deviceLinkCreatePairing(mailboxServer: string | null): Promise<string>;
  deviceLinkAcceptPairing(code: string): Promise<void>;
  deviceLinkAwaitStep(
    dbPath: string,
    dbKey: string
  ): Promise<import("../bindings").AccountResult | null>;
  deviceLinkReset(): Promise<void>;
  linkCreatePairing(mailboxServer: string | null): Promise<string>;
  linkAcceptPairing(code: string): Promise<void>;
  linkSendBundleStep(): Promise<boolean>;

  // Core messaging
  sendDm(recipientDid: string, plaintext: number[], sentAtMs: number): Promise<void>;
  sendGroupMessage(groupId: string, plaintext: number[], sentAtMs: number): Promise<void>;
  sendMessageWithAttachments(
    target: import("../bindings").MessageTarget,
    body: string,
    attachments: import("../bindings").AttachmentFfi[],
    previews: import("../bindings").LinkPreviewFfi[],
    sentAtMs: number
  ): Promise<void>;
  nextEvents(): Promise<import("../bindings").IncomingEvent[]>;
  saveMessage(msg: import("../bindings").StoredMessageFfi): Promise<void>;
  loadConversations(): Promise<import("../bindings").ConversationSummaryFfi[]>;
  loadMessages(conversationId: string): Promise<import("../bindings").StoredMessageFfi[]>;
  markMessagesRead(conversationId: string, upToSentAtMs: number): Promise<number>;
  unreadCount(conversationId: string): Promise<number>;
  receiveMessages(): Promise<import("../bindings").DecryptedMessage[]>;
  sendReadReceipt(recipientDid: string, timestamps: number[]): Promise<void>;

  // Identity / contacts
  did(): Promise<string>;
  deviceId(): Promise<number>;
  ownDisplayName(): Promise<string>;
  setDisplayName(displayName: string): Promise<void>;
  hasRecovery(): Promise<boolean>;
  updateRecoveryBlob(prfOutput: number[], servers: string[]): Promise<void>;
  homeServer(): Promise<string>;
  generateRecoveryPhrase(): Promise<string>;
  recoveryPhraseToSeed(phrase: string): Promise<number[]>;
  deriveDidFromPasskey(prfOutput: number[], signupServerUrl: string): Promise<string>;
  contactDisplayName(did: string): Promise<string>;
  getAccountInfo(did: string): Promise<import("../bindings").AccountInfoFfi>;
  refreshContactProfile(did: string): Promise<boolean>;
  listContacts(): Promise<import("../bindings").ContactRowFfi[]>;
  touchContact(did: string, curated: boolean): Promise<void>;
  fetchAndCacheProfile(did: string, profileKey: Uint8Array): Promise<void>;
  primeContactProfile(did: string, displayName: string, profileKey: Uint8Array): Promise<void>;
  blockContact(did: string): Promise<void>;
  unblockContact(did: string): Promise<void>;

  // Message requests / safety
  acceptRequest(did: string): Promise<void>;
  deleteRequest(did: string): Promise<void>;
  setPendingRequest(did: string, pending: boolean): Promise<void>;
  reportAndBlock(did: string, reason: string): Promise<void>;
  listBlocked(): Promise<import("../bindings").ContactRowFfi[]>;

  // Disappearing-message timers
  getConversationTimer(conversationId: string): Promise<number | null>;
  setConversationTimer(recipientDid: string, expirySecs: number | null): Promise<void>;
  deleteExpiredMessages(): Promise<string[]>;

  // Account lifecycle
  leaveServer(): Promise<void>;
  deleteIdentity(): Promise<void>;

  // Session management
  clearSession(): Promise<void>;

  // Projects
  fetchProjects(): Promise<import("../bindings").ProjectInfoFfi[]>;
  requestProjectToken(projectUrl: string): Promise<string>;
  validateInvite(token: string): Promise<import("../bindings").InviteInfo>;

  // Connection state
  connectionState(): Promise<import("../bindings").ConnectionState>;
  waitForConnectionStateChange(last: import("../bindings").ConnectionState): Promise<import("../bindings").ConnectionState>;

  // Groups
  createGroup(title: string, description: string, expirySeconds: number): Promise<import("../bindings").CreatedGroupFfi>;
  fetchGroupState(groupId: string): Promise<import("../bindings").GroupSummaryFfi>;
  cachedGroupState(groupId: string): Promise<import("../bindings").GroupSummaryFfi | null>;
  inviteMember(groupId: string, recipientDid: string, role: number): Promise<void>;
  acceptInvite(groupId: string): Promise<void>;
  declineInvite(groupId: string): Promise<void>;
  cancelJoinRequest(groupId: string): Promise<void>;
  approveJoinRequest(groupId: string, encryptedMemberId: string): Promise<void>;
  denyJoinRequest(groupId: string, encryptedMemberId: string): Promise<void>;
  removeMember(groupId: string, encryptedMemberId: string): Promise<void>;
  leaveGroup(groupId: string): Promise<void>;
  isGroupMember(groupId: string): Promise<boolean>;
  changeMemberRole(groupId: string, encryptedMemberId: string, newRole: number): Promise<void>;
  setGroupExpiry(groupId: string, expirySeconds: number): Promise<void>;
  setGroupTitle(groupId: string, newTitle: string): Promise<void>;
  groupExpirySeconds(groupId: string): Promise<number>;
  applyPendingGroupChanges(groupId: string): Promise<number>;
  listGroups(): Promise<string[]>;
  joinViaLink(
    masterKey: number[],
    hostingServerUrl: string,
    password: number[]
  ): Promise<import("../bindings").JoinResultFfi>;

  // Reactions / edit / delete
  sendReaction(
    target: import("../bindings").MessageTarget,
    targetAuthor: string,
    targetSentAtMs: number,
    emoji: string,
    remove: boolean,
    sentAtMs: number
  ): Promise<void>;
  sendEdit(
    target: import("../bindings").MessageTarget,
    targetSentAtMs: number,
    newBody: string,
    sentAtMs: number
  ): Promise<void>;
  sendDelete(
    target: import("../bindings").MessageTarget,
    targetAuthor: string,
    targetSentAtMs: number,
    forEveryone: boolean,
    sentAtMs: number
  ): Promise<void>;
  loadReactions(conversationId: string): Promise<import("../bindings").ReactionFfi[]>;
  loadMessageRevisions(conversationId: string, author: string, sentAtMs: number): Promise<import("../bindings").MessageRevisionFfi[]>;

  // Attachments / link previews / external links
  uploadAttachment(
    plaintext: number[],
    contentType: string,
    fileName: string | null,
    width: number,
    height: number,
    durationMs: number,
    thumbnail: number[],
    flags: number
  ): Promise<import("../bindings").AttachmentFfi>;
  downloadAttachment(attachment: import("../bindings").AttachmentFfi): Promise<number[]>;
  openExternal(url: string): Promise<void>;
  fetchLinkPreview(url: string): Promise<import("../bindings").LinkPreviewMetaFfi>;
}
