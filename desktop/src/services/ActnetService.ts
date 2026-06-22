// Service layer — mirrors iOS ActnetService.swift + AppCoreProtocol.
// On Desktop, the factory methods (createAccount/login) set up state in the
// Rust backend; subsequent calls operate on that global state.

export enum ServiceMode {
  Mock = "mock",
  DevServer = "devServer",
}

// ── Serialized FFI types (JSON round-trip from Rust) ─────────────────────────

export interface StoredMessageFfi {
  id: string;
  conversationId: string;
  senderDid: string;
  body: string;
  sentAtMs: number;
  editedAtMs: number | null;
  readAtMs: number | null;
  deliveryStatus: number;
  editCount: number;
  deleted: boolean;
  kind: number;
  metadata: string | null;
  expireTimerSecs: number;
  expireAtMs: number | null;
}

export interface ConversationSummaryFfi {
  conversationId: string;
  groupTitle: string | null;
  lastMessage: StoredMessageFfi | null;
  isRequest: boolean;
  isBlocked: boolean;
}

export interface AccountInfoFfi {
  did: string;
  displayName: string | null;
  isBot: boolean;
}

export interface ProjectInfoFfi {
  name: string;
  url: string;
  description: string;
}

export interface ContactRowFfi {
  did: string;
  displayName: string;
  curated: boolean;
}

export interface ReactionFfi {
  conversationId: string;
  targetAuthor: string;
  targetSentAtMs: number;
  reactorDid: string;
  emoji: string;
  reactedAtMs: number;
}

export interface MessageRevisionFfi {
  body: string;
  replacedAtMs: number;
}

export interface AccountResult {
  did: string;
  displayName: string;
}

// ConnectionState — mirrors the Rust enum
export type ConnectionState =
  | { type: "connected" }
  | { type: "connecting" }
  | { type: "disconnected" }
  | { type: "reconnecting"; nextAttemptAtMs: number };

// IncomingEvent — mirrors the Rust IncomingEvent enum
export type IncomingEvent =
  | { type: "message"; msg: StoredMessageFfi }
  | {
      type: "receiptUpdate";
      conversationId: string;
      sentAtMs: number;
      deliveryStatus: number;
    }
  | { type: "groupInvite"; groupId: string }
  | { type: "groupMetadataChanged"; groupId: string }
  | { type: "storageSynced" }
  | {
      type: "messageEdited";
      conversationId: string;
      authorDid: string;
      sentAtMs: number;
      newBody: string;
      editedAtMs: number;
    }
  | {
      type: "messageDeleted";
      conversationId: string;
      authorDid: string;
      sentAtMs: number;
    }
  | {
      type: "reactionUpdated";
      conversationId: string;
      targetAuthor: string;
      targetSentAtMs: number;
      reactorDid: string;
      emoji: string;
      removed: boolean;
    }
  | { type: "messagesExpired"; conversationIds: string[] };

export interface GroupSummaryFfi {
  groupId: string;
  revision: number;
  title: string;
  description: string;
  expirySeconds: number;
}

export interface CreatedGroupFfi {
  groupId: string;
}

// ── Service interface ─────────────────────────────────────────────────────────

export interface ActnetService {
  // Account factory
  createAccount(
    serverUrl: string,
    dbPath: string,
    dbKey: string,
    displayName: string,
    inviteToken: string | null
  ): Promise<AccountResult>;
  login(dbPath: string, dbKey: string): Promise<AccountResult>;
  recoverFromBlob(
    serverUrl: string,
    did: string,
    dbPath: string,
    dbKey: string,
    displayName: string
  ): Promise<AccountResult>;

  // Core messaging
  sendDm(recipientDid: string, body: string, sentAtMs: number): Promise<void>;
  sendGroupMessage(
    groupId: string,
    body: string,
    sentAtMs: number
  ): Promise<void>;
  receiveMessages(): Promise<StoredMessageFfi[]>;
  nextEvents(): Promise<IncomingEvent[]>;
  saveMessage(msg: StoredMessageFfi): Promise<void>;
  loadConversations(): Promise<ConversationSummaryFfi[]>;
  loadMessages(conversationId: string): Promise<StoredMessageFfi[]>;
  markMessagesRead(
    conversationId: string,
    upToSentAtMs: number
  ): Promise<number>;
  unreadCount(conversationId: string): Promise<number>;

  // Identity / contacts
  did(): Promise<string>;
  deviceId(): Promise<number>;
  ownDisplayName(): Promise<string>;
  setDisplayName(displayName: string): Promise<void>;
  hasRecovery(): Promise<boolean>;
  contactDisplayName(did: string): Promise<string>;
  getAccountInfo(did: string): Promise<AccountInfoFfi>;
  refreshContactProfile(did: string): Promise<boolean>;
  listContacts(): Promise<ContactRowFfi[]>;
  touchContact(did: string, curated: boolean): Promise<void>;
  fetchAndCacheProfile(did: string, profileKey: Uint8Array): Promise<void>;
  primeContactProfile(
    did: string,
    displayName: string,
    profileKey: Uint8Array
  ): Promise<void>;
  blockContact(did: string): Promise<void>;
  unblockContact(did: string): Promise<void>;

  // Account lifecycle
  leaveServer(): Promise<void>;
  deleteIdentity(): Promise<void>;

  // Projects
  fetchProjects(): Promise<ProjectInfoFfi[]>;
  requestProjectToken(projectUrl: string): Promise<string>;
  validateInvite(token: string): Promise<import("../models").InviteInfo>;

  // Connection state (long-poll — resolves only when state changes)
  connectionState(): Promise<ConnectionState>;
  waitForConnectionStateChange(last: ConnectionState): Promise<ConnectionState>;

  // Groups
  createGroup(
    title: string,
    description: string,
    expirySeconds: number
  ): Promise<CreatedGroupFfi>;
  fetchGroupState(groupId: string): Promise<GroupSummaryFfi>;
  cachedGroupState(groupId: string): Promise<GroupSummaryFfi | null>;
  inviteMember(
    groupId: string,
    recipientDid: string,
    role: number
  ): Promise<void>;
  acceptInvite(groupId: string): Promise<void>;
  declineInvite(groupId: string): Promise<void>;
  cancelJoinRequest(groupId: string): Promise<void>;
  approveJoinRequest(
    groupId: string,
    encryptedMemberId: string
  ): Promise<void>;
  denyJoinRequest(groupId: string, encryptedMemberId: string): Promise<void>;
  removeMember(groupId: string, encryptedMemberId: string): Promise<void>;
  leaveGroup(groupId: string): Promise<void>;
  isGroupMember(groupId: string): Promise<boolean>;
  changeMemberRole(
    groupId: string,
    encryptedMemberId: string,
    newRole: number
  ): Promise<void>;
  setGroupExpiry(groupId: string, expirySeconds: number): Promise<void>;
  setGroupTitle(groupId: string, newTitle: string): Promise<void>;
  groupExpirySeconds(groupId: string): Promise<number>;
  applyPendingGroupChanges(groupId: string): Promise<number>;
  listGroups(): Promise<string[]>;

  // Reactions / edit / delete
  sendReaction(
    target: { type: "dm"; recipientDid: string } | { type: "group"; groupId: string },
    targetAuthor: string,
    targetSentAtMs: number,
    emoji: string,
    remove: boolean,
    sentAtMs: number
  ): Promise<void>;
  sendEdit(
    target: { type: "dm"; recipientDid: string } | { type: "group"; groupId: string },
    targetSentAtMs: number,
    newBody: string,
    sentAtMs: number
  ): Promise<void>;
  sendDelete(
    target: { type: "dm"; recipientDid: string } | { type: "group"; groupId: string },
    targetAuthor: string,
    targetSentAtMs: number,
    forEveryone: boolean,
    sentAtMs: number
  ): Promise<void>;
  loadReactions(conversationId: string): Promise<ReactionFfi[]>;
  loadMessageRevisions(
    conversationId: string,
    author: string,
    sentAtMs: number
  ): Promise<MessageRevisionFfi[]>;
}
