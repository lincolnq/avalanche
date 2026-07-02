import type { Account, Conversation, InviteInfo } from "../models";
import type { Message } from "../models/Message";
import type {
  ServiceMode,
  AvalancheService,
  ConnectionState,
  ReactionFfi,
  MessageRevisionFfi,
  JoinResultFfi,
  ContactRowFfi,
  AttachmentFfi,
  LinkPreviewFfi,
  LinkPreviewMetaFfi,
} from "../services/AvalancheService";

// ── Persisted account shape (stored in tauri-plugin-store) ────────────────────

export interface PersistedAccount {
  did: string;
  displayName: string;
  dbPath: string;
  servers: Array<{ id: string; name: string; url: string }>;
}

// ── Store shape ───────────────────────────────────────────────────────────────

export interface AppStore {
  accounts: Account[];
  isOnboarding: boolean;
  // True while the onboarding flow is being run to ADD an account to an already
  // signed-in session ("Sign in to another account"). Distinct from
  // isOnboarding (first-run / signed-out): the main UI stays mounted underneath
  // and the existing accounts' loops keep running. Cleared by enterApp.
  isAddingAccount: boolean;
  serviceMode: ServiceMode;
  selectedTab: "chats" | "network";
  conversations: Conversation[];
  messagesByConversation: Record<string, Message[]>;
  reactionsByConversation: Record<string, ReactionFfi[]>;
  connectionStates: Record<string, ConnectionState>;
  pendingInviteToken: string | null;
  serverUrl: string;
  // Desktop-only (T72): close button hides the window to the tray instead of
  // quitting, so the WS + notifications survive. Persisted; the Rust
  // CloseRequested handler reads the same key from the plugin-store file.
  closeToTray: boolean;
}

// ── Session guards ────────────────────────────────────────────────────────────

// Cross-concern load-once / lifecycle guard collections, deliberately
// non-reactive (never read in a tracking scope for rendering). Owned by the
// provider composition root and passed by reference into each state module;
// resetSession clears them all. Kept in one named object so every cross-module
// mutation is greppable as `guards.`.
export interface SessionGuards {
  // True once the merged conversation list has been loaded this session.
  loadedConversations: { value: boolean };
  // Conversation ids whose message timelines have been loaded.
  loadedMessages: Set<string>;
  // Conversation ids whose reactions have been loaded.
  loadedReactions: Set<string>;
  // Conversation ids created in-memory (e.g. an incoming DM in a brand-new
  // thread) that aren't yet backed by a row in the local DB.
  // loadConversationsFromStore preserves only these across a reload, NOT
  // arbitrary DB-absent entries, which would resurrect conversations the DB
  // intentionally dropped. The incoming-message handler persists the received
  // message (so the conversation appears in the DB summaries on the next
  // reload), and this set bridges the brief gap until that reload runs; the
  // drop-on-DB-appearance path then hands it back to normal lifecycle.
  pendingConversations: Set<string>;
}

// ── Context value ─────────────────────────────────────────────────────────────

export interface AppContextValue {
  store: AppStore;
  // Account-less service for factory + pure calls (createAccount, validateInvite,
  // recoveryPhraseToSeed, …). Per-account calls go through serviceFor(accountId).
  service: () => AvalancheService;
  serviceFor: (accountId: string) => AvalancheService;
  setSelectedTab: (tab: "chats" | "network") => void;
  createAccount: (
    serverUrl: string,
    serverName: string,
    displayName: string,
    inviteToken: string | null,
    prfOutput: number[]
  ) => Promise<void>;
  restoreAccounts: () => Promise<void>;
  logout: () => void;
  serverUrl: () => string;
  setServerUrl: (url: string) => void;
  // Close-to-tray preference (T72) + manual reconnect (offline banner action).
  closeToTray: () => boolean;
  setCloseToTray: (on: boolean) => void;
  reconnectNow: () => void;
  joinServer: (
    serverUrl: string,
    serverName: string,
    existingAccountId: string
  ) => Promise<void>;
  sendMessage: (
    conversationId: string,
    text: string,
    recipientDid: string,
    senderAccountId: string
  ) => Promise<void>;
  sendGroupMessage: (conversation: Conversation, text: string) => Promise<void>;
  sendMessageWithAttachments: (
    conversation: Conversation,
    text: string,
    attachments: AttachmentFfi[],
    previews: LinkPreviewFfi[]
  ) => Promise<void>;
  uploadAttachment: (
    accountId: string,
    plaintext: number[],
    contentType: string,
    fileName: string | null,
    width: number,
    height: number,
    durationMs: number,
    thumbnail: number[],
    flags: number
  ) => Promise<AttachmentFfi>;
  downloadAttachment: (accountId: string, attachment: AttachmentFfi) => Promise<number[]>;
  fetchLinkPreview: (url: string) => Promise<LinkPreviewMetaFfi>;
  openExternal: (url: string) => Promise<void>;
  loadConversationsFromStore: () => Promise<void>;
  loadMessagesFromStore: (conversationId: string, accountId: string) => void;
  markAllMessagesRead: (conversationId: string, accountId: string) => void;
  findOrCreateDMConversation: (
    recipientDid: string,
    accountId: string
  ) => Conversation;
  aggregateConnectionState: () => ConnectionState;
  unreadCount: (conversation: Conversation) => number;
  displayName: (did: string, accountId: string) => string;
  isBot: (did: string, accountId: string) => boolean;
  setPendingInviteToken: (token: string | null) => void;
  validateInvite: (token: string) => Promise<InviteInfo>;

  // Conversation selection (lifted so compose/group flows can open a chat)
  selectedConversationId: () => string | null;
  selectConversation: (id: string | null) => void;
  reloadConversations: () => Promise<void>;
  // Reactive: latest group whose metadata changed (T74 membership re-check).
  groupMetaChange: () => { groupId: string; n: number };

  // Track A — message actions
  reactionsFor: (conversation: Conversation, message: Message) => ReactionFfi[];
  loadReactions: (conversationId: string) => void;
  toggleReaction: (conversation: Conversation, message: Message, emoji: string) => void;
  editMessage: (conversation: Conversation, message: Message, newBody: string) => void;
  loadMessageRevisions: (conversation: Conversation, message: Message) => Promise<MessageRevisionFfi[]>;
  deleteMessage: (conversation: Conversation, message: Message, forEveryone: boolean) => void;
  retryMessage: (conversation: Conversation, message: Message) => Promise<void>;

  // Track B — groups + join
  createGroupAndOpen: (
    accountId: string,
    title: string,
    recipientDids: string[],
    expirySeconds: number
  ) => Promise<Conversation>;
  joinViaLink: (
    accountId: string,
    masterKey: number[],
    hostingServerUrl: string,
    password: number[]
  ) => Promise<JoinResultFfi>;
  leaveGroup: (conversation: Conversation) => Promise<void>;

  // Track D — safety + timers
  acceptRequest: (conversation: Conversation) => Promise<void>;
  deleteRequest: (conversation: Conversation) => Promise<void>;
  reportAndBlock: (conversation: Conversation, reason: string) => Promise<void>;
  blockContact: (accountId: string, did: string) => Promise<void>;
  unblockContact: (accountId: string, did: string) => Promise<void>;
  // Aggregated across all accounts; each row carries its owning accountId.
  listBlocked: () => Promise<Array<ContactRowFfi & { accountId: string }>>;
  getConversationTimer: (accountId: string, conversationId: string) => Promise<number | null>;
  setConversationTimer: (
    accountId: string,
    recipientDid: string,
    expirySecs: number | null
  ) => Promise<void>;

  // Track E — settings / account lifecycle
  setAccountDisplayName: (accountId: string, displayName: string) => Promise<void>;
  leaveServer: (accountId: string) => Promise<void>;
  deleteIdentity: (accountId: string) => Promise<void>;
  hasRecovery: (accountId: string) => Promise<boolean>;
  generateRecoveryPhrase: () => Promise<string>;
  recoverFromPhrase: (phrase: string, serverUrl: string, displayName: string) => Promise<void>;
  // "Sign in to another account" — additive onboarding over the live session.
  startAddAccount: () => void;
  cancelAddAccount: () => void;

  // Device linking (T71). New-device side (onboarding, account-less):
  // deviceLinkShowCode (show mode) or deviceLinkEnterCode (paste mode), then
  // deviceLinkComplete polls until the account arrives (then persists + enters).
  // Existing-device side (settings, signed in): linkShowCode or linkEnterCode,
  // then linkSendBundle polls until the bundle is delivered.
  deviceLinkShowCode: () => Promise<string>;
  deviceLinkEnterCode: (code: string) => Promise<void>;
  deviceLinkComplete: () => Promise<void>;
  deviceLinkCancel: () => Promise<void>;
  linkShowCode: (accountId: string) => Promise<string>;
  linkEnterCode: (accountId: string, code: string) => Promise<void>;
  linkSendBundle: (accountId: string) => Promise<void>;

  // Deep links — route a pasted/opened link (conversation/<did>, i/<token>)
  isDeepLink: (raw: string) => boolean;
  handleDeepLink: (raw: string) => void;
}
