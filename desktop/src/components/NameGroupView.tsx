import { createSignal, For, Show } from "solid-js";
import { FiArrowLeft } from "solid-icons/fi";
import { useApp } from "../state/AppContext";
import DisappearingMessagesPicker from "./DisappearingMessagesPicker";
import "./NameGroupView.css";

interface Props {
  // The identity that will own + create the group (chosen in NewConversationView).
  accountId: string;
  memberDids: string[];
  onBack: () => void;
  onClose: () => void;
}

/**
 * "New group" naming step, pushed from the compose flow's New Group action.
 * Collects the group name and disappearing-messages timer, shows a read-only
 * member preview, then creates the group and opens its thread. Mirrors the iOS
 * `NameGroupView`. Group photos / server selection are intentionally omitted
 * (the core has no avatar param; create_group uses the account's pinned server).
 */
export default function NameGroupView(props: Props) {
  const app = useApp();
  const accountId = (): string => props.accountId;

  const [name, setName] = createSignal("");
  const [expirySeconds, setExpirySeconds] = createSignal(0);
  const [creating, setCreating] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const canCreate = (): boolean => name().trim().length > 0 && !creating();

  async function create() {
    if (!canCreate()) return;
    setCreating(true);
    setError(null);
    try {
      const conv = await app.createGroupAndOpen(
        accountId(),
        name().trim(),
        props.memberDids,
        expirySeconds()
      );
      app.selectConversation(conv.id);
      props.onClose();
    } catch (e) {
      setCreating(false);
      setError(e instanceof Error ? e.message : "Couldn't create the group.");
    }
  }

  return (
    <div class="name-group">
      <div class="name-group-header">
        <button class="back-btn" onClick={props.onBack} aria-label="Back">
          <FiArrowLeft size={16} />
          Back
        </button>
        <span class="name-group-title">New group</span>
        <span class="name-group-header-spacer" />
      </div>

      <div class="name-group-body scrollbar-thin">
        <input
          class="text-input name-group-name"
          type="text"
          value={name()}
          placeholder="Group name (required)"
          onInput={(e) => setName(e.currentTarget.value)}
          disabled={creating()}
        />

        <div class="name-group-section">
          <div class="name-group-section-label">Disappearing messages</div>
          <DisappearingMessagesPicker
            seconds={expirySeconds()}
            disabled={creating()}
            onChange={(s) => setExpirySeconds(s)}
          />
        </div>

        <div class="name-group-section">
          <div class="name-group-section-label">
            Members ({props.memberDids.length})
          </div>
          <Show
            when={props.memberDids.length > 0}
            fallback={
              <div class="name-group-empty">
                No members yet — you can add people after creating the group.
              </div>
            }
          >
            <For each={props.memberDids}>
              {(did) => (
                <div class="name-group-member">
                  {app.displayName(did, accountId())}
                </div>
              )}
            </For>
          </Show>
        </div>

        <Show when={error()}>
          <div class="form-error">{error()}</div>
        </Show>
      </div>

      <div class="name-group-actions">
        <button class="btn-primary" disabled={!canCreate()} onClick={create}>
          {creating() ? "Creating…" : "Create"}
        </button>
      </div>
    </div>
  );
}
