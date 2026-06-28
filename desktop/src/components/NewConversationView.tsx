import { createSignal, onMount, For, Show } from "solid-js";
import { FiX } from "solid-icons/fi";
import { useApp } from "../state/AppContext";
import type { ContactRowFfi } from "../services/AvalancheService";
import RecipientTokenField from "./RecipientTokenField";
import NameGroupView from "./NameGroupView";
import "./NewConversationView.css";

interface Props {
  onClose: () => void;
}

/**
 * New-message composer modal. A recipient chip field plus a browsable contact
 * list, and two actions: **Message** (enabled at exactly one recipient; opens
 * the DM thread) and **New Group** (one or more recipients; routes to the
 * NameGroup step). Mirrors the iOS `ComposeMessageView`.
 */
export default function NewConversationView(props: Props) {
  const app = useApp();
  const accountId = (): string => app.store.accounts[0]?.id ?? "";

  const [chips, setChips] = createSignal<string[]>([]);
  const [contacts, setContacts] = createSignal<ContactRowFfi[]>([]);
  const [showGroup, setShowGroup] = createSignal(false);

  onMount(() => {
    void (async () => {
      try {
        const rows = await app.service().listContacts();
        setContacts(rows);
      } catch {
        setContacts([]);
      }
    })();
  });

  function addChip(did: string) {
    const v = did.trim();
    // A pasted contact/invite link (avalanche:// or go.theavalanche.net,
    // /conversation/<did> or /i/<token>) is routed via the deep-link handler —
    // it opens the DM (or onboarding for an off-server invite) and closes this
    // modal. Matches iOS RecipientTokenField, which accepts the same links.
    if (app.isDeepLink(v)) {
      app.handleDeepLink(v);
      props.onClose();
      return;
    }
    // Otherwise only raw DIDs are valid recipients — names are picked from the
    // contact list below. This stops a free-typed name (e.g. "Alice") from
    // being committed as a chip and creating a DM keyed on a non-DID string.
    if (!v.startsWith("did:")) return;
    setChips((prev) => (prev.includes(v) ? prev : [...prev, v]));
  }

  function removeChip(did: string) {
    setChips((prev) => prev.filter((d) => d !== did));
  }

  const availableContacts = (): ContactRowFfi[] =>
    contacts().filter((c) => !chips().includes(c.did));

  const canMessage = (): boolean => chips().length === 1;
  const canGroup = (): boolean => chips().length >= 1;

  function messageTapped() {
    if (!canMessage()) return;
    const conv = app.findOrCreateDMConversation(chips()[0], accountId());
    app.selectConversation(conv.id);
    props.onClose();
  }

  return (
    <div class="newconv-backdrop" onClick={props.onClose}>
      <div class="newconv" onClick={(e) => e.stopPropagation()}>
        <Show
          when={!showGroup()}
          fallback={
            <NameGroupView
              memberDids={chips()}
              onBack={() => setShowGroup(false)}
              onClose={props.onClose}
            />
          }
        >
          <div class="newconv-header">
            <span class="newconv-title">New message</span>
            <button
              class="newconv-close"
              onClick={props.onClose}
              aria-label="Close"
            >
              <FiX size={18} />
            </button>
          </div>

          <div class="newconv-recipients">
            <RecipientTokenField
              chips={chips()}
              onAdd={addChip}
              onRemove={removeChip}
              displayName={(did) => app.displayName(did, accountId())}
              placeholder="Type a name or DID"
            />
          </div>

          <div class="newconv-contacts scrollbar-thin">
            <Show
              when={availableContacts().length > 0}
              fallback={
                <div class="newconv-empty">No more contacts to add.</div>
              }
            >
              <For each={availableContacts()}>
                {(c) => (
                  <button
                    class="newconv-contact"
                    onClick={() => addChip(c.did)}
                  >
                    <span class="newconv-contact-name">
                      {app.displayName(c.did, accountId())}
                    </span>
                    <span class="newconv-contact-did">{c.did}</span>
                  </button>
                )}
              </For>
            </Show>
          </div>

          <div class="newconv-actions">
            <button
              class="btn-secondary"
              disabled={!canMessage()}
              onClick={messageTapped}
            >
              Message
            </button>
            <button
              class="btn-primary"
              disabled={!canGroup()}
              onClick={() => setShowGroup(true)}
            >
              New Group
            </button>
          </div>
        </Show>
      </div>
    </div>
  );
}
