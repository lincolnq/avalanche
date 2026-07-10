import { createSignal, onMount, Show } from "solid-js";
import { FiMoreHorizontal } from "solid-icons/fi";
import { useApp } from "../state/AppContext";
import type { SharedContactFfi } from "../bindings";
import ContactAvatar from "./ContactAvatar";
import { copyContact } from "../lib/contactClipboard";
import "./SharedContactCard.css";

interface Props {
  contact: SharedContactFfi;
  // The owning conversation's account — save/curate routes to its core.
  accountId: string;
  // Own outgoing copy (plum bubble) vs a received card (incoming tone). Own
  // copies show no Save action — the sender already has the contact.
  mine: boolean;
  // Compose-staging variant: shows a dismiss (×), no Save / menu.
  staged?: boolean;
  onDismiss?: () => void;
}

/**
 * A shared contact card rendered inside a message bubble (docs/35). Shows the
 * name the sender knows the person by, plus a "Save" action that adds them to
 * the recipient's contact book. A menu offers "Message" (open a DM) and "Copy
 * contact" (re-share). Mirrors iOS/Android SharedContactCard.
 */
export default function SharedContactCard(props: Props) {
  const app = useApp();
  const [menuOpen, setMenuOpen] = createSignal(false);
  // Driven by the real contact book: a DID already curated shows "Saved".
  const [saved, setSaved] = createSignal(false);
  const displayName = () => props.contact.name.trim() || props.contact.did.slice(-8);

  onMount(() => {
    // Load whether this DID is already a curated contact so the card renders
    // "Saved" instead of an active Save button (received cards only).
    if (props.mine || props.staged) return;
    void app
      .listContacts(props.accountId)
      .then((rows) => {
        if (rows.some((r) => r.did === props.contact.did && r.isCurated)) setSaved(true);
      })
      .catch(() => {});
  });

  function save() {
    // Optimistically flip to "Saved", then persist.
    setSaved(true);
    void app.saveSharedContact(props.accountId, props.contact.did, props.contact.name);
  }

  function message() {
    setMenuOpen(false);
    const conv = app.findOrCreateDMConversation(props.contact.did, props.accountId);
    app.setSelectedTab("chats");
    app.selectConversation(conv.id);
  }

  function copy() {
    setMenuOpen(false);
    copyContact({ did: props.contact.did, name: props.contact.name });
  }

  return (
    <div class="shared-contact-card" classList={{ mine: props.mine }}>
      <ContactAvatar
        name={displayName()}
        did={props.contact.did}
        accountId={props.accountId}
        isBot={false}
      />
      <div class="shared-contact-info">
        <span class="shared-contact-name">{displayName()}</span>
        <span class="shared-contact-label">Contact</span>
      </div>

      <Show when={props.staged}>
        <button
          class="shared-contact-dismiss"
          aria-label="Remove contact"
          onClick={() => props.onDismiss?.()}
        >
          ×
        </button>
      </Show>

      <Show when={!props.staged && !props.mine}>
        <Show
          when={saved()}
          fallback={
            <button class="shared-contact-save" onClick={save}>
              Save
            </button>
          }
        >
          <span class="shared-contact-saved">✓ Saved</span>
        </Show>
      </Show>

      <Show when={!props.staged}>
        <button
          class="shared-contact-menu-btn"
          aria-label="Contact actions"
          onClick={() => setMenuOpen(true)}
        >
          <FiMoreHorizontal size={14} />
        </button>
        <Show when={menuOpen()}>
          <div class="context-menu-backdrop" onClick={() => setMenuOpen(false)} />
          <div class="context-menu" classList={{ mine: props.mine }}>
            <button class="ctx-item" onClick={message}>
              Message {displayName()}
            </button>
            <button class="ctx-item" onClick={copy}>
              Copy contact
            </button>
          </div>
        </Show>
      </Show>
    </div>
  );
}
