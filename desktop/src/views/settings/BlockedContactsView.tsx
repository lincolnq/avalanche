import { createSignal, onMount, For, Show } from "solid-js";
import { FiX } from "solid-icons/fi";
import { useApp } from "../../state/AppContext";
import type { ContactRowFfi } from "../../services/AvalancheService";
import "./BlockedContactsView.css";

interface Props {
  onClose: () => void;
}

export default function BlockedContactsView(props: Props) {
  const app = useApp();
  const [rows, setRows] = createSignal<ContactRowFfi[]>([]);
  const [loading, setLoading] = createSignal(true);

  async function refresh() {
    setLoading(true);
    setRows(await app.listBlocked());
    setLoading(false);
  }

  onMount(() => {
    void refresh();
  });

  async function unblock(did: string) {
    await app.unblockContact(did);
    await refresh();
  }

  return (
    <div class="blocked-backdrop" onClick={props.onClose}>
      <div class="blocked-sheet" onClick={(e) => e.stopPropagation()}>
        <div class="blocked-header">
          <span>Blocked Contacts</span>
          <button class="blocked-close" onClick={props.onClose} aria-label="Close">
            <FiX size={18} />
          </button>
        </div>
        <div class="blocked-body scrollbar-thin">
          <Show when={!loading()} fallback={<div class="blocked-loading">Loading…</div>}>
            <Show
              when={rows().length > 0}
              fallback={<div class="blocked-empty">No blocked contacts.</div>}
            >
              <For each={rows()}>
                {(c) => (
                  <div class="blocked-row">
                    <div class="blocked-info">
                      <div class="blocked-name">{c.displayName || c.did}</div>
                      <div class="blocked-did">{c.did}</div>
                    </div>
                    <button
                      class="btn-secondary blocked-unblock"
                      onClick={() => void unblock(c.did)}
                    >
                      Unblock
                    </button>
                  </div>
                )}
              </For>
            </Show>
          </Show>
        </div>
      </div>
    </div>
  );
}
