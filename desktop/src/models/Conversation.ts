export interface Conversation {
  id: string;
  title: string;
  accountId: string;       // owning account DID — NOT ownerId
  serverUrl: string;
  recipientDid?: string;   // DM only: other party's DID
  groupId?: string;        // group only: URL-safe-no-pad base64 group id
  lastMessage?: string;
  lastMessageDate?: number; // unix-ms, NOT a Date object
  lastMessageKind: number;
  lastMessageMetadata?: string;
  lastMessageSenderDid?: string;
  isGroup: boolean;
  isRequest: boolean;
  isBlocked: boolean;
  // Authoritative unread count seeded from ConversationSummaryFfi.unreadCount at
  // load (core excludes own + expired messages). Backs the chat-list badge for
  // conversations whose transcript isn't loaded yet; once loaded, per-message
  // read state takes over. Optimistically cleared to 0 on open. (A5)
  unreadCount?: number;
  // Group only: true once the user has left (or was removed). The conversation
  // stays visible (read-only) instead of vanishing — the composer is replaced
  // with a "no longer a member" notice, Signal-style.
  hasLeft?: boolean;
}

export function groupConversationId(groupId: string): string {
  return `group-${groupId}`;
}
