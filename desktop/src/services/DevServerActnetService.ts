import { invoke } from "@tauri-apps/api/core";
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

export class DevServerActnetService implements ActnetService {
  async createAccount(
    serverUrl: string,
    dbPath: string,
    dbKey: string,
    displayName: string,
    inviteToken: string | null
  ): Promise<AccountResult> {
    return invoke("create_account", {
      serverUrl,
      dbPath,
      dbKey,
      displayName,
      inviteToken,
    });
  }

  async login(dbPath: string, dbKey: string): Promise<AccountResult> {
    return invoke("login", { dbPath, dbKey });
  }

  async recoverFromBlob(
    serverUrl: string,
    did: string,
    dbPath: string,
    dbKey: string,
    displayName: string
  ): Promise<AccountResult> {
    return invoke("recover_from_blob", {
      serverUrl,
      did,
      dbPath,
      dbKey,
      displayName,
    });
  }

  async sendDm(
    recipientDid: string,
    body: string,
    sentAtMs: number
  ): Promise<void> {
    return invoke("send_dm", { recipientDid, body, sentAtMs });
  }

  async sendGroupMessage(
    groupId: string,
    body: string,
    sentAtMs: number
  ): Promise<void> {
    return invoke("send_group_message", { groupId, body, sentAtMs });
  }

  async receiveMessages(): Promise<StoredMessageFfi[]> {
    return invoke("receive_messages");
  }

  async nextEvents(): Promise<IncomingEvent[]> {
    return invoke("next_events");
  }

  async saveMessage(msg: StoredMessageFfi): Promise<void> {
    return invoke("save_message", { msg });
  }

  async loadConversations(): Promise<ConversationSummaryFfi[]> {
    return invoke("load_conversations");
  }

  async loadMessages(conversationId: string): Promise<StoredMessageFfi[]> {
    return invoke("load_messages", { conversationId });
  }

  async markMessagesRead(
    conversationId: string,
    upToSentAtMs: number
  ): Promise<number> {
    return invoke("mark_messages_read", { conversationId, upToSentAtMs });
  }

  async unreadCount(conversationId: string): Promise<number> {
    return invoke("unread_count", { conversationId });
  }

  async did(): Promise<string> {
    return invoke("did");
  }

  async deviceId(): Promise<number> {
    return invoke("device_id");
  }

  async ownDisplayName(): Promise<string> {
    return invoke("own_display_name");
  }

  async setDisplayName(displayName: string): Promise<void> {
    return invoke("set_display_name", { displayName });
  }

  async hasRecovery(): Promise<boolean> {
    return invoke("has_recovery");
  }

  async contactDisplayName(did: string): Promise<string> {
    return invoke("contact_display_name", { did });
  }

  async getAccountInfo(did: string): Promise<AccountInfoFfi> {
    return invoke("get_account_info", { did });
  }

  async refreshContactProfile(did: string): Promise<boolean> {
    return invoke("refresh_contact_profile", { did });
  }

  async listContacts(): Promise<ContactRowFfi[]> {
    return invoke("list_contacts");
  }

  async touchContact(did: string, curated: boolean): Promise<void> {
    return invoke("touch_contact", { did, curated });
  }

  async fetchAndCacheProfile(
    did: string,
    profileKey: Uint8Array
  ): Promise<void> {
    return invoke("fetch_and_cache_profile", {
      did,
      profileKey: Array.from(profileKey),
    });
  }

  async primeContactProfile(
    did: string,
    displayName: string,
    profileKey: Uint8Array
  ): Promise<void> {
    return invoke("prime_contact_profile", {
      did,
      displayName,
      profileKey: Array.from(profileKey),
    });
  }

  async blockContact(did: string): Promise<void> {
    return invoke("block_contact", { did });
  }

  async unblockContact(did: string): Promise<void> {
    return invoke("unblock_contact", { did });
  }

  async leaveServer(): Promise<void> {
    return invoke("leave_server");
  }

  async deleteIdentity(): Promise<void> {
    return invoke("delete_identity");
  }

  async fetchProjects(): Promise<ProjectInfoFfi[]> {
    return invoke("fetch_projects");
  }

  async requestProjectToken(projectUrl: string): Promise<string> {
    return invoke("request_project_token", { projectUrl });
  }

  async validateInvite(token: string): Promise<InviteInfo> {
    return invoke("validate_invite", { token });
  }

  async connectionState(): Promise<ConnectionState> {
    return invoke("connection_state");
  }

  async waitForConnectionStateChange(
    last: ConnectionState
  ): Promise<ConnectionState> {
    return invoke("wait_for_connection_state_change", { last });
  }

  async createGroup(
    title: string,
    description: string,
    expirySeconds: number
  ): Promise<CreatedGroupFfi> {
    return invoke("create_group", { title, description, expirySeconds });
  }

  async fetchGroupState(groupId: string): Promise<GroupSummaryFfi> {
    return invoke("fetch_group_state", { groupId });
  }

  async cachedGroupState(groupId: string): Promise<GroupSummaryFfi | null> {
    return invoke("cached_group_state", { groupId });
  }

  async inviteMember(
    groupId: string,
    recipientDid: string,
    role: number
  ): Promise<void> {
    return invoke("invite_member", { groupId, recipientDid, role });
  }

  async acceptInvite(groupId: string): Promise<void> {
    return invoke("accept_invite", { groupId });
  }

  async declineInvite(groupId: string): Promise<void> {
    return invoke("decline_invite", { groupId });
  }

  async cancelJoinRequest(groupId: string): Promise<void> {
    return invoke("cancel_join_request", { groupId });
  }

  async approveJoinRequest(
    groupId: string,
    encryptedMemberId: string
  ): Promise<void> {
    return invoke("approve_join_request", { groupId, encryptedMemberId });
  }

  async denyJoinRequest(
    groupId: string,
    encryptedMemberId: string
  ): Promise<void> {
    return invoke("deny_join_request", { groupId, encryptedMemberId });
  }

  async removeMember(
    groupId: string,
    encryptedMemberId: string
  ): Promise<void> {
    return invoke("remove_member", { groupId, encryptedMemberId });
  }

  async leaveGroup(groupId: string): Promise<void> {
    return invoke("leave_group", { groupId });
  }

  async isGroupMember(groupId: string): Promise<boolean> {
    return invoke("is_group_member", { groupId });
  }

  async changeMemberRole(
    groupId: string,
    encryptedMemberId: string,
    newRole: number
  ): Promise<void> {
    return invoke("change_member_role", { groupId, encryptedMemberId, newRole });
  }

  async setGroupExpiry(groupId: string, expirySeconds: number): Promise<void> {
    return invoke("set_group_expiry", { groupId, expirySeconds });
  }

  async setGroupTitle(groupId: string, newTitle: string): Promise<void> {
    return invoke("set_group_title", { groupId, newTitle });
  }

  async groupExpirySeconds(groupId: string): Promise<number> {
    return invoke("group_expiry_seconds", { groupId });
  }

  async applyPendingGroupChanges(groupId: string): Promise<number> {
    return invoke("apply_pending_group_changes", { groupId });
  }

  async listGroups(): Promise<string[]> {
    return invoke("list_groups");
  }

  async sendReaction(
    target: { type: "dm"; recipientDid: string } | { type: "group"; groupId: string },
    targetAuthor: string,
    targetSentAtMs: number,
    emoji: string,
    remove: boolean,
    sentAtMs: number
  ): Promise<void> {
    return invoke("send_reaction", {
      target,
      targetAuthor,
      targetSentAtMs,
      emoji,
      remove,
      sentAtMs,
    });
  }

  async sendEdit(
    target: { type: "dm"; recipientDid: string } | { type: "group"; groupId: string },
    targetSentAtMs: number,
    newBody: string,
    sentAtMs: number
  ): Promise<void> {
    return invoke("send_edit", { target, targetSentAtMs, newBody, sentAtMs });
  }

  async sendDelete(
    target: { type: "dm"; recipientDid: string } | { type: "group"; groupId: string },
    targetAuthor: string,
    targetSentAtMs: number,
    forEveryone: boolean,
    sentAtMs: number
  ): Promise<void> {
    return invoke("send_delete", {
      target,
      targetAuthor,
      targetSentAtMs,
      forEveryone,
      sentAtMs,
    });
  }

  async loadReactions(conversationId: string): Promise<ReactionFfi[]> {
    return invoke("load_reactions", { conversationId });
  }

  async loadMessageRevisions(
    conversationId: string,
    author: string,
    sentAtMs: number
  ): Promise<MessageRevisionFfi[]> {
    return invoke("load_message_revisions", { conversationId, author, sentAtMs });
  }
}
