import { createSignal, onMount, For, Show, Switch, Match } from "solid-js";
import { FiX } from "solid-icons/fi";
import { useApp } from "../state/AppContext";
import type { Conversation } from "../models";
import type {
  GroupSummaryFfi,
  GroupMemberFfi,
  GroupPendingFfi,
} from "../services/AvalancheService";
import DisappearingMessagesPicker, {
  disappearingLabel,
} from "./DisappearingMessagesPicker";
import "./GroupDetailView.css";

interface Props {
  conversation: Conversation;
  onClose: () => void;
}

const ROLE_ADMIN = 1;
const ROLE_MEMBER = 0;

/**
 * Group info modal (docs/03-groups.md): title + revision, disappearing-message
 * timer, member list with admin role changes, pending approvals/invites, and
 * leave-group. Admin-only controls render only when the current user is an
 * admin; the server enforces the same. Mirrors the iOS `GroupDetailView`.
 */
export default function GroupDetailView(props: Props) {
  const app = useApp();
  // This group's owning account drives every group call + the "is this me?" check.
  const accountId = (): string => props.conversation.accountId;
  const svc = () => app.serviceFor(props.conversation.accountId);
  const groupId = props.conversation.groupId;

  const [summary, setSummary] = createSignal<GroupSummaryFfi | null>(null);
  const [loading, setLoading] = createSignal(true);
  const [renaming, setRenaming] = createSignal(false);
  const [renameText, setRenameText] = createSignal("");

  // groupId is required; a conversation without one shouldn't open this modal,
  // but guard so the rest of the component can treat it as a definite string.
  if (groupId === undefined) {
    props.onClose();
    return null;
  }
  const gid: string = groupId;

  async function load() {
    setLoading(true);
    try {
      setSummary(await svc().fetchGroupState(gid));
    } catch {
      const cached = await svc().cachedGroupState(gid).catch(() => null);
      setSummary(cached);
    } finally {
      setLoading(false);
    }
  }

  /** Re-fetch after a mutation; falls back to cached state if the fetch 404s. */
  async function reload() {
    try {
      setSummary(await svc().fetchGroupState(gid));
    } catch {
      const cached = await svc().cachedGroupState(gid).catch(() => null);
      if (cached) setSummary(cached);
    }
  }

  onMount(() => {
    void load();
  });

  const myMember = (): GroupMemberFfi | undefined =>
    summary()?.members.find((m) => m.did === accountId());
  const amAdmin = (): boolean => myMember()?.role === ROLE_ADMIN;
  const amMember = (): boolean => myMember() !== undefined;

  /** Current user sorts first; everyone else keeps server order. */
  function orderedMembers(members: GroupMemberFfi[]): GroupMemberFfi[] {
    return [...members].sort((a, b) =>
      a.did === accountId() ? -1 : b.did === accountId() ? 1 : 0
    );
  }

  function memberName(member: GroupMemberFfi): string {
    return member.did === accountId()
      ? "You"
      : app.displayName(member.did, accountId());
  }

  async function saveRename() {
    const trimmed = renameText().trim();
    const s = summary();
    if (!trimmed || trimmed === s?.title) {
      setRenaming(false);
      return;
    }
    try {
      await svc().setGroupTitle(gid, trimmed);
      await reload();
      // Refresh the conversation list so the sidebar + header title update too;
      // reload() only refreshes this modal's local summary.
      await app.reloadConversations();
    } catch (e) {
      console.warn("setGroupTitle failed:", e);
    }
    setRenaming(false);
  }

  async function setExpiry(seconds: number) {
    try {
      await svc().setGroupExpiry(gid, seconds);
    } catch (e) {
      console.warn("setGroupExpiry failed:", e);
    }
    // Reload either way: reverts the picker to the server's value on failure.
    await reload();
  }

  async function changeRole(member: GroupMemberFfi, newRole: number) {
    try {
      await svc().changeMemberRole(gid, member.encryptedMemberId, newRole);
      await reload();
    } catch (e) {
      console.warn("changeMemberRole failed:", e);
    }
  }

  async function approve(pending: GroupPendingFfi) {
    try {
      await svc().approveJoinRequest(gid, pending.encryptedMemberId);
      await reload();
    } catch (e) {
      console.warn("approveJoinRequest failed:", e);
    }
  }

  async function deny(pending: GroupPendingFfi) {
    try {
      await svc().denyJoinRequest(gid, pending.encryptedMemberId);
      await reload();
    } catch (e) {
      console.warn("denyJoinRequest failed:", e);
    }
  }

  async function leave() {
    // Marks the conversation read-only (hasLeft) and keeps it visible instead
    // of deleting it — ConversationView swaps the composer for a notice. Only
    // closes on success; a failed leave keeps the modal open (still a member).
    try {
      await app.leaveGroup(props.conversation);
      props.onClose();
    } catch {
      // leaveGroup already logged; stay open so the user sees they're still in.
    }
  }

  return (
    <div class="groupdetail-backdrop" onClick={props.onClose}>
      <div class="groupdetail" onClick={(e) => e.stopPropagation()}>
        <div class="groupdetail-header">
          <span class="groupdetail-title">Group info</span>
          <button
            class="groupdetail-close"
            onClick={props.onClose}
            aria-label="Close"
          >
            <FiX size={18} />
          </button>
        </div>

        <div class="groupdetail-body scrollbar-thin">
          <Switch>
            <Match when={loading() && summary() === null}>
              <div class="groupdetail-loading">Loading…</div>
            </Match>
            <Match when={summary() === null}>
              <div class="groupdetail-loading">Couldn't load this group.</div>
            </Match>
            <Match when={summary()}>
              {(s) => (
                <>
                  {/* Title + revision */}
                  <section class="groupdetail-section">
                    <Show
                      when={renaming()}
                      fallback={
                        <div class="groupdetail-title-row">
                          <span class="groupdetail-group-name">
                            {s().title || "Group"}
                          </span>
                          <Show when={amAdmin()}>
                            <button
                              class="groupdetail-link-btn"
                              onClick={() => {
                                setRenameText(s().title);
                                setRenaming(true);
                              }}
                            >
                              Rename
                            </button>
                          </Show>
                        </div>
                      }
                    >
                      <div class="groupdetail-rename">
                        <input
                          class="text-input"
                          type="text"
                          value={renameText()}
                          placeholder="Group name"
                          onInput={(e) => setRenameText(e.currentTarget.value)}
                        />
                        <div class="groupdetail-rename-actions">
                          <button
                            class="groupdetail-link-btn"
                            onClick={() => setRenaming(false)}
                          >
                            Cancel
                          </button>
                          <button
                            class="groupdetail-link-btn"
                            disabled={renameText().trim().length === 0}
                            onClick={() => void saveRename()}
                          >
                            Save
                          </button>
                        </div>
                      </div>
                    </Show>
                    <div class="groupdetail-revision">
                      Revision {s().revision}
                    </div>
                  </section>

                  {/* Disappearing messages */}
                  <section class="groupdetail-section">
                    <div class="groupdetail-section-label">
                      Disappearing messages
                    </div>
                    <Show
                      when={amAdmin()}
                      fallback={
                        <div class="groupdetail-readonly">
                          {disappearingLabel(s().expirySeconds)}
                        </div>
                      }
                    >
                      <DisappearingMessagesPicker
                        seconds={s().expirySeconds}
                        onChange={(secs) => void setExpiry(secs)}
                      />
                    </Show>
                  </section>

                  {/* Members */}
                  <section class="groupdetail-section">
                    <div class="groupdetail-section-label">
                      Members ({s().members.length})
                    </div>
                    <For each={orderedMembers(s().members)}>
                      {(member) => (
                        <div class="groupdetail-member">
                          <span class="groupdetail-member-name">
                            {memberName(member)}
                          </span>
                          <Show when={member.role === ROLE_ADMIN}>
                            <span class="groupdetail-badge">Admin</span>
                          </Show>
                          <span class="groupdetail-spacer" />
                          <Show
                            when={amAdmin() && member.did !== accountId()}
                          >
                            <Show
                              when={member.role === ROLE_ADMIN}
                              fallback={
                                <button
                                  class="groupdetail-link-btn"
                                  onClick={() =>
                                    void changeRole(member, ROLE_ADMIN)
                                  }
                                >
                                  Make admin
                                </button>
                              }
                            >
                              <button
                                class="groupdetail-link-btn"
                                onClick={() =>
                                  void changeRole(member, ROLE_MEMBER)
                                }
                              >
                                Remove admin
                              </button>
                            </Show>
                          </Show>
                        </div>
                      )}
                    </For>
                  </section>

                  {/* Pending approvals (admin-only) */}
                  <Show
                    when={amAdmin() && s().pendingApprovals.length > 0}
                  >
                    <section class="groupdetail-section">
                      <div class="groupdetail-section-label">
                        Pending approvals ({s().pendingApprovals.length})
                      </div>
                      <For each={s().pendingApprovals}>
                        {(p) => (
                          <div class="groupdetail-pending">
                            <span class="groupdetail-pending-id">
                              {p.encryptedMemberId}
                            </span>
                            <span class="groupdetail-spacer" />
                            <button
                              class="groupdetail-link-btn"
                              onClick={() => void approve(p)}
                            >
                              Approve
                            </button>
                            <button
                              class="groupdetail-link-btn danger"
                              onClick={() => void deny(p)}
                            >
                              Deny
                            </button>
                          </div>
                        )}
                      </For>
                    </section>
                  </Show>

                  {/* Pending invites (read-only) */}
                  <Show when={s().pendingInvites.length > 0}>
                    <section class="groupdetail-section">
                      <div class="groupdetail-section-label">
                        Pending invites ({s().pendingInvites.length})
                      </div>
                      <For each={s().pendingInvites}>
                        {(p) => (
                          <div class="groupdetail-pending-id readonly">
                            {p.encryptedMemberId}
                          </div>
                        )}
                      </For>
                    </section>
                  </Show>

                  {/* Leave group */}
                  <Show when={amMember()}>
                    <section class="groupdetail-section">
                      <button
                        class="groupdetail-leave"
                        onClick={() => void leave()}
                      >
                        Leave group
                      </button>
                    </section>
                  </Show>
                </>
              )}
            </Match>
          </Switch>
        </div>
      </div>
    </div>
  );
}
