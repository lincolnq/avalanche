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
}

export function groupConversationId(groupId: string): string {
  return `group-${groupId}`;
}
