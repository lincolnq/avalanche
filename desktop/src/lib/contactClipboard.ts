import { createSignal } from "solid-js";
import type { SharedContactFfi } from "../bindings";

/**
 * In-app "copy contact → paste into a message" clipboard (docs/35).
 *
 * iOS and Android put the copied card on the OS pasteboard under a private type
 * ({did,name} JSON). The desktop webview can't reliably carry a private
 * clipboard MIME type, so the copied card is held in this module-level reactive
 * signal instead — scoped to the running app session. Copy from a group-member
 * row or a received contact card; the composer surfaces a "Paste contact"
 * affordance while it's set, and staging/sending consumes it.
 */
const [copiedContact, setCopiedContact] = createSignal<SharedContactFfi | null>(null);

export { copiedContact };

export function copyContact(contact: SharedContactFfi): void {
  setCopiedContact({ did: contact.did, name: contact.name });
}

export function clearCopiedContact(): void {
  setCopiedContact(null);
}
