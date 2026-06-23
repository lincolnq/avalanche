import type {
  ActnetService,
  AccountResult,
  StoredMessageFfi,
  ConversationSummaryFfi,
  AccountInfoFfi,
  ProjectInfoFfi,
  ConnectionState,
  IncomingEvent,
  ContactRowFfi,
  ReactionFfi,
  MessageRevisionFfi,
  GroupSummaryFfi,
  CreatedGroupFfi,
} from "./ActnetService";
import type { InviteInfo } from "../models";

const MOCK_SERVER_URL = "https://mock.avalancheapp.net";
const MOCK_SERVER_NAME = "Mock Server";

function makeMockDid(): string {
  return `did:plc:mock${Math.random().toString(36).slice(2, 10)}`;
}

// Seed conversations that appear after mock login/create.
export function seedConversations(
  accountId: string
): ConversationSummaryFfi[] {
  const now = Date.now();
  return [
    {
      conversationId: `group-mockgroup1`,
      groupTitle: "General",
      lastMessage: {
        id: "msg-g1-1",
        conversationId: "group-mockgroup1",
        senderDid: "did:plc:organizer",
        body: "Welcome to the server!",
        sentAtMs: now - 60_000,
        editedAtMs: null,
        readAtMs: null,
        deliveryStatus: 1,
        editCount: 0,
        deleted: false,
        kind: 0,
        metadata: null,
        expireTimerSecs: 0,
        expireAtMs: null,
      },
      isRequest: false,
      isBlocked: false,
    },
    {
      conversationId: `group-mockgroup2`,
      groupTitle: "Announcements",
      lastMessage: {
        id: "msg-g2-1",
        conversationId: "group-mockgroup2",
        senderDid: "did:plc:organizer",
        body: "Rally this Saturday at 10am",
        sentAtMs: now - 3_600_000,
        editedAtMs: null,
        readAtMs: null,
        deliveryStatus: 1,
        editCount: 0,
        deleted: false,
        kind: 0,
        metadata: null,
        expireTimerSecs: 0,
        expireAtMs: null,
      },
      isRequest: false,
      isBlocked: false,
    },
    {
      conversationId: `dm-${accountId}-did:plc:organizer`,
      groupTitle: null,
      lastMessage: {
        id: "msg-dm-1",
        conversationId: `dm-${accountId}-did:plc:organizer`,
        senderDid: "did:plc:organizer",
        body: "Hey, welcome aboard!",
        sentAtMs: now - 120_000,
        editedAtMs: null,
        readAtMs: null,
        deliveryStatus: 1,
        editCount: 0,
        deleted: false,
        kind: 0,
        metadata: null,
        expireTimerSecs: 0,
        expireAtMs: null,
      },
      isRequest: false,
      isBlocked: false,
    },
  ];
}

// Seed messages for a conversation.
function seedMessages(conversationId: string, accountId: string): StoredMessageFfi[] {
  const now = Date.now();
  if (conversationId === "group-mockgroup1") {
    return [
      {
        id: "msg-g1-0",
        conversationId,
        senderDid: "did:plc:organizer",
        body: "Hey everyone!",
        sentAtMs: now - 3_700_000,
        editedAtMs: null,
        readAtMs: now,
        deliveryStatus: 3,
        editCount: 0,
        deleted: false,
        kind: 0,
        metadata: null,
        expireTimerSecs: 0,
        expireAtMs: null,
      },
      {
        id: "msg-g1-1",
        conversationId,
        senderDid: "did:plc:organizer",
        body: "Welcome to the server!",
        sentAtMs: now - 60_000,
        editedAtMs: null,
        readAtMs: null,
        deliveryStatus: 1,
        editCount: 0,
        deleted: false,
        kind: 0,
        metadata: null,
        expireTimerSecs: 0,
        expireAtMs: null,
      },
    ];
  }
  if (conversationId === "group-mockgroup2") {
    return [
      {
        id: "msg-g2-1",
        conversationId,
        senderDid: "did:plc:organizer",
        body: "Rally this Saturday at 10am",
        sentAtMs: now - 3_600_000,
        editedAtMs: null,
        readAtMs: null,
        deliveryStatus: 1,
        editCount: 0,
        deleted: false,
        kind: 0,
        metadata: null,
        expireTimerSecs: 0,
        expireAtMs: null,
      },
    ];
  }
  if (conversationId.startsWith(`dm-${accountId}-`)) {
    return [
      {
        id: "msg-dm-0",
        conversationId,
        senderDid: "did:plc:organizer",
        body: "Hi there! Glad you joined.",
        sentAtMs: now - 500_000,
        editedAtMs: null,
        readAtMs: now,
        deliveryStatus: 3,
        editCount: 0,
        deleted: false,
        kind: 0,
        metadata: null,
        expireTimerSecs: 0,
        expireAtMs: null,
      },
      {
        id: "msg-dm-1",
        conversationId,
        senderDid: "did:plc:organizer",
        body: "Hey, welcome aboard!",
        sentAtMs: now - 120_000,
        editedAtMs: null,
        readAtMs: null,
        deliveryStatus: 1,
        editCount: 0,
        deleted: false,
        kind: 0,
        metadata: null,
        expireTimerSecs: 0,
        expireAtMs: null,
      },
    ];
  }
  return [];
}

export class MockActnetService implements ActnetService {
  private mockDid = "";
  private storedMessages: Map<string, StoredMessageFfi[]> = new Map();
  private pendingEvents: IncomingEvent[] = [];
  private nextEventsResolve: ((events: IncomingEvent[]) => void) | null = null;

  private pushEvent(ev: IncomingEvent) {
    if (this.nextEventsResolve) {
      const resolve = this.nextEventsResolve;
      this.nextEventsResolve = null;
      resolve([ev]);
    } else {
      this.pendingEvents.push(ev);
    }
  }

  private echoReply(conversationId: string, senderDid: string, body: string) {
    setTimeout(() => {
      this.pushEvent({
        type: "message",
        msg: {
          id: `echo-${Date.now()}`,
          conversationId,
          senderDid,
          body,
          sentAtMs: Date.now(),
          editedAtMs: null,
          readAtMs: null,
          deliveryStatus: 1,
          editCount: 0,
          deleted: false,
          kind: 0,
          metadata: null,
          expireTimerSecs: 0,
          expireAtMs: null,
        },
      });
    }, 1000);
  }

  async createAccount(
    _serverUrl: string,
    _dbPath: string,
    _dbKey: string,
    displayName: string,
    _inviteToken: string | null
  ): Promise<AccountResult> {
    await new Promise((r) => setTimeout(r, 500));
    this.mockDid = makeMockDid();
    return { did: this.mockDid, displayName };
  }

  async login(_dbPath: string, _dbKey: string): Promise<AccountResult> {
    this.mockDid = this.mockDid || makeMockDid();
    return { did: this.mockDid, displayName: "Me" };
  }

  async recoverFromBlob(
    _serverUrl: string,
    did: string,
    _dbPath: string,
    _dbKey: string,
    displayName: string
  ): Promise<AccountResult> {
    await new Promise((r) => setTimeout(r, 500));
    this.mockDid = did;
    return { did, displayName };
  }

  async sendDm(recipientDid: string, body: string, sentAtMs: number): Promise<void> {
    await new Promise((r) => setTimeout(r, 100));
    void sentAtMs;
    const convId = `dm-${this.mockDid}-${recipientDid}`;
    this.echoReply(convId, recipientDid, body);
  }

  async sendGroupMessage(
    groupId: string,
    body: string,
    _sentAtMs: number
  ): Promise<void> {
    await new Promise((r) => setTimeout(r, 100));
    // AppContext strips the "group-" prefix before passing groupId here,
    // but conversationIds in the store retain the full "group-<id>" form.
    this.echoReply(`group-${groupId}`, "did:plc:organizer", body);
  }

  async receiveMessages(): Promise<StoredMessageFfi[]> {
    return [];
  }

  nextEvents(): Promise<IncomingEvent[]> {
    if (this.pendingEvents.length > 0) {
      return Promise.resolve(this.pendingEvents.splice(0));
    }
    return new Promise((resolve) => {
      this.nextEventsResolve = resolve;
    });
  }

  async saveMessage(msg: StoredMessageFfi): Promise<void> {
    const existing = this.storedMessages.get(msg.conversationId) ?? [];
    this.storedMessages.set(msg.conversationId, [...existing, msg]);
  }

  async loadConversations(): Promise<ConversationSummaryFfi[]> {
    return seedConversations(this.mockDid);
  }

  async loadMessages(conversationId: string): Promise<StoredMessageFfi[]> {
    if (!this.storedMessages.has(conversationId)) {
      this.storedMessages.set(
        conversationId,
        seedMessages(conversationId, this.mockDid)
      );
    }
    return this.storedMessages.get(conversationId) ?? [];
  }

  async markMessagesRead(
    conversationId: string,
    upToSentAtMs: number
  ): Promise<number> {
    const msgs = this.storedMessages.get(conversationId) ?? [];
    const now = Date.now();
    let count = 0;
    const updated = msgs.map((m) => {
      if (
        m.readAtMs === null &&
        m.senderDid !== this.mockDid &&
        m.sentAtMs <= upToSentAtMs
      ) {
        count++;
        return { ...m, readAtMs: now };
      }
      return m;
    });
    this.storedMessages.set(conversationId, updated);
    return count;
  }

  async unreadCount(conversationId: string): Promise<number> {
    const msgs = this.storedMessages.get(conversationId) ?? [];
    return msgs.filter((m) => m.readAtMs === null && m.senderDid !== this.mockDid)
      .length;
  }

  async did(): Promise<string> { return this.mockDid; }
  async deviceId(): Promise<number> { return 1; }
  async ownDisplayName(): Promise<string> { return "Me"; }
  async setDisplayName(_displayName: string): Promise<void> {}
  async hasRecovery(): Promise<boolean> { return false; }
  async contactDisplayName(did: string): Promise<string> {
    if (did === "did:plc:organizer") return "Jamie (Organizer)";
    return "";
  }
  async getAccountInfo(did: string): Promise<AccountInfoFfi> {
    return { did, displayName: null, isBot: false };
  }
  async refreshContactProfile(_did: string): Promise<boolean> { return false; }
  async listContacts(): Promise<ContactRowFfi[]> { return []; }
  async touchContact(_did: string, _curated: boolean): Promise<void> {}
  async fetchAndCacheProfile(_did: string, _profileKey: Uint8Array): Promise<void> {}
  async primeContactProfile(
    _did: string,
    _displayName: string,
    _profileKey: Uint8Array
  ): Promise<void> {}
  async blockContact(_did: string): Promise<void> {}
  async unblockContact(_did: string): Promise<void> {}
  async leaveServer(): Promise<void> {}
  async deleteIdentity(): Promise<void> {}

  async fetchProjects(): Promise<ProjectInfoFfi[]> {
    return [
      {
        name: "Testbot",
        url: "http://localhost:3001",
        description: "Chat with an AI bot",
      },
    ];
  }

  async requestProjectToken(_projectUrl: string): Promise<string> {
    return `mock-token-${Math.random().toString(36).slice(2, 10)}`;
  }

  async validateInvite(token: string): Promise<InviteInfo> {
    return {
      token,
      serverUrl: MOCK_SERVER_URL,
      serverName: MOCK_SERVER_NAME,
      inviterDid: undefined,
      inviterDisplayName: undefined,
      postOnboardingRedirect: undefined,
    };
  }

  async connectionState(): Promise<ConnectionState> {
    return { type: "connected" };
  }

  waitForConnectionStateChange(
    _last: ConnectionState
  ): Promise<ConnectionState> {
    // Never changes in mock mode — park forever.
    return new Promise(() => {});
  }

  async createGroup(
    title: string,
    _description: string,
    _expirySeconds: number
  ): Promise<CreatedGroupFfi> {
    return { groupId: `mockgrp-${title.slice(0, 8)}-${Date.now()}` };
  }

  async fetchGroupState(groupId: string): Promise<GroupSummaryFfi> {
    return {
      groupId,
      revision: 0,
      title: "Mock Group",
      description: "",
      expirySeconds: 0,
    };
  }

  async cachedGroupState(_groupId: string): Promise<GroupSummaryFfi | null> {
    return null;
  }

  async inviteMember(
    _groupId: string,
    _recipientDid: string,
    _role: number
  ): Promise<void> {}
  async acceptInvite(_groupId: string): Promise<void> {}
  async declineInvite(_groupId: string): Promise<void> {}
  async cancelJoinRequest(_groupId: string): Promise<void> {}
  async approveJoinRequest(
    _groupId: string,
    _encryptedMemberId: string
  ): Promise<void> {}
  async denyJoinRequest(
    _groupId: string,
    _encryptedMemberId: string
  ): Promise<void> {}
  async removeMember(
    _groupId: string,
    _encryptedMemberId: string
  ): Promise<void> {}
  async leaveGroup(_groupId: string): Promise<void> {}
  async isGroupMember(_groupId: string): Promise<boolean> { return true; }
  async changeMemberRole(
    _groupId: string,
    _encryptedMemberId: string,
    _newRole: number
  ): Promise<void> {}
  async setGroupExpiry(_groupId: string, _expirySeconds: number): Promise<void> {}
  async setGroupTitle(_groupId: string, _newTitle: string): Promise<void> {}
  async groupExpirySeconds(_groupId: string): Promise<number> { return 0; }
  async applyPendingGroupChanges(_groupId: string): Promise<number> { return 0; }
  async listGroups(): Promise<string[]> { return []; }

  async sendReaction(
    _target: { type: "dm"; recipientDid: string } | { type: "group"; groupId: string },
    _targetAuthor: string,
    _targetSentAtMs: number,
    _emoji: string,
    _remove: boolean,
    _sentAtMs: number
  ): Promise<void> {}
  async sendEdit(
    _target: { type: "dm"; recipientDid: string } | { type: "group"; groupId: string },
    _targetSentAtMs: number,
    _newBody: string,
    _sentAtMs: number
  ): Promise<void> {}
  async sendDelete(
    _target: { type: "dm"; recipientDid: string } | { type: "group"; groupId: string },
    _targetAuthor: string,
    _targetSentAtMs: number,
    _forEveryone: boolean,
    _sentAtMs: number
  ): Promise<void> {}
  async loadReactions(_conversationId: string): Promise<ReactionFfi[]> {
    return [];
  }
  async loadMessageRevisions(
    _conversationId: string,
    _author: string,
    _sentAtMs: number
  ): Promise<MessageRevisionFfi[]> {
    return [];
  }
}
