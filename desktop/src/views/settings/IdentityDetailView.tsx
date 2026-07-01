import { createSignal, Show } from "solid-js";
import { FiArrowLeft, FiCopy, FiCheck, FiSlash, FiSmartphone, FiChevronRight } from "solid-icons/fi";
import { useApp } from "../../state/AppContext";
import AccountAvatar from "../../components/AccountAvatar";
import BlockedContactsView from "./BlockedContactsView";
import { contactInviteUrl } from "../../lib/format";
import type { Account } from "../../models";
import "./IdentityDetailView.css";

interface Props {
  account: Account;
  onBack: () => void;
  // Open the per-identity "Link a device" flow for this account.
  onLinkDevice: () => void;
}

/**
 * Identity detail: edit display name, copy DID + contact link, view the home
 * server, link a device, open blocked contacts, and delete the identity. Mirrors
 * iOS IdentityDetailView, minus the QR image (desktop has no QR rendering — see
 * the documented QR divergence). Link-a-device is per-identity here (Day 7
 * multi-account), reached via onLinkDevice.
 */
export default function IdentityDetailView(props: Props) {
  const { setAccountDisplayName, deleteIdentity } = useApp();

  const [name, setName] = createSignal(props.account.displayName);
  const [savingName, setSavingName] = createSignal(false);
  const [showBlocked, setShowBlocked] = createSignal(false);
  const [confirming, setConfirming] = createSignal(false);
  const [deleting, setDeleting] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [copied, setCopied] = createSignal<"did" | "link" | null>(null);

  const homeServer = () => props.account.servers[0];
  const contactUrl = () => {
    const s = homeServer();
    return s ? contactInviteUrl(s.url, props.account.id) : null;
  };
  const nameDirty = () => name().trim() !== props.account.displayName && name().trim().length > 0;

  async function copy(text: string, which: "did" | "link") {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(which);
      setTimeout(() => setCopied(null), 1500);
    } catch (e) {
      console.warn("clipboard write failed:", e);
    }
  }

  async function saveName() {
    if (!nameDirty()) return;
    setSavingName(true);
    setError(null);
    try {
      await setAccountDisplayName(props.account.id, name());
    } catch (e) {
      setError(e instanceof Error ? e.message : "Couldn't update display name");
    }
    setSavingName(false);
  }

  async function doDelete() {
    setDeleting(true);
    setError(null);
    try {
      await deleteIdentity(props.account.id);
      // deleteIdentity drops this account (back to onboarding if it was the last).
    } catch (e) {
      setError(e instanceof Error ? e.message : "Couldn't delete identity");
      setDeleting(false);
      setConfirming(false);
    }
  }

  return (
    <div class="identity-detail">
      <header class="settings-subheader">
        <button class="back-btn" onClick={props.onBack}>
          <FiArrowLeft size={14} />Back
        </button>
        <h1>Identity</h1>
      </header>

      <div class="identity-detail-body scrollbar-thin">
        <div class="identity-head">
          <AccountAvatar name={props.account.displayName} did={props.account.id} />
        </div>

        <label class="identity-field">
          <span class="identity-label">Display name</span>
          <div class="identity-name-row">
            <input
              class="text-input identity-name-input"
              value={name()}
              onInput={(e) => setName(e.currentTarget.value)}
              spellcheck={false}
            />
            <button
              class="btn-primary identity-save-btn"
              onClick={() => void saveName()}
              disabled={!nameDirty() || savingName()}
            >
              {savingName() ? "Saving…" : "Save"}
            </button>
          </div>
        </label>

        <div class="identity-field">
          <span class="identity-label">DID</span>
          <button class="identity-copy-row" onClick={() => void copy(props.account.id, "did")}>
            <span class="identity-mono">{props.account.id}</span>
            {copied() === "did" ? <FiCheck size={15} /> : <FiCopy size={15} />}
          </button>
        </div>

        <Show when={contactUrl()}>
          {(url) => (
            <div class="identity-field">
              <span class="identity-label">Contact link</span>
              <button class="identity-copy-row" onClick={() => void copy(url(), "link")}>
                <span class="identity-mono">{url()}</span>
                {copied() === "link" ? <FiCheck size={15} /> : <FiCopy size={15} />}
              </button>
            </div>
          )}
        </Show>

        <Show when={homeServer()}>
          {(s) => (
            <div class="identity-field">
              <span class="identity-label">Home server</span>
              <div class="identity-server-card">
                <span class="identity-server-name">{s().name}</span>
                <span class="identity-mono identity-server-url">{s().url}</span>
              </div>
            </div>
          )}
        </Show>

        <p class="identity-note">
          Your home server is listed publicly so people can reach you. Your display
          name, other server memberships, contacts, and messages are not public.
        </p>

        <button class="identity-row-btn" onClick={() => props.onLinkDevice()}>
          <FiSmartphone size={16} />
          <span>Link a device</span>
          <FiChevronRight size={16} class="identity-row-chevron" />
        </button>

        <button class="identity-row-btn" onClick={() => setShowBlocked(true)}>
          <FiSlash size={16} />
          <span>Blocked Contacts</span>
        </button>

        <Show when={error()}>
          <p class="settings-error">{error()}</p>
        </Show>

        <div class="identity-delete">
          <Show
            when={confirming()}
            fallback={
              <button class="btn-danger" onClick={() => setConfirming(true)}>
                Delete identity
              </button>
            }
          >
            <p class="identity-delete-warning">
              Delete {props.account.displayName} from {props.account.servers.length} server
              {props.account.servers.length === 1 ? "" : "s"} and mark it deleted in the
              public registry. This cannot be undone.
            </p>
            <div class="identity-confirm-actions">
              <button class="btn-secondary" onClick={() => setConfirming(false)} disabled={deleting()}>
                Cancel
              </button>
              <button class="btn-danger" onClick={() => void doDelete()} disabled={deleting()}>
                {deleting() ? "Deleting…" : "Delete"}
              </button>
            </div>
          </Show>
        </div>
      </div>

      <Show when={showBlocked()}>
        <BlockedContactsView onClose={() => setShowBlocked(false)} />
      </Show>
    </div>
  );
}
