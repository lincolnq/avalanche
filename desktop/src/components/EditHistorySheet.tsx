import { createSignal, onMount, For, Show } from "solid-js";
import { FiX } from "solid-icons/fi";
import { useApp } from "../state/AppContext";
import type { Conversation, Message } from "../models";
import type { MessageRevisionFfi } from "../services/AvalancheService";
import { formatTime } from "../lib/format";
import "./EditHistorySheet.css";

interface Props {
  conversation: Conversation;
  message: Message;
  onClose: () => void;
}

export default function EditHistorySheet(props: Props) {
  const app = useApp();
  const [revisions, setRevisions] = createSignal<MessageRevisionFfi[]>([]);
  const [loading, setLoading] = createSignal(true);

  onMount(() => {
    void app
      .loadMessageRevisions(props.conversation, props.message)
      .then((r) => {
        setRevisions(r);
        setLoading(false);
      });
  });

  return (
    <div class="sheet-backdrop" onClick={props.onClose}>
      <div class="sheet" onClick={(e) => e.stopPropagation()}>
        <div class="sheet-header">
          <span>Edit History</span>
          <button class="sheet-close" onClick={props.onClose} aria-label="Close">
            <FiX size={18} />
          </button>
        </div>
        <div class="sheet-body scrollbar-thin">
          <Show when={!loading()} fallback={<div class="sheet-loading">Loading…</div>}>
            <For each={revisions()}>
              {(rev) => (
                <div class="revision-row">
                  <div class="revision-label">Edited</div>
                  <div class="revision-body">{rev.body}</div>
                  <div class="revision-time">{formatTime(rev.replacedAtMs)}</div>
                </div>
              )}
            </For>
            <div class="revision-row current">
              <div class="revision-label">Current</div>
              <div class="revision-body">{props.message.body}</div>
              <Show when={props.message.editedAtMs !== undefined}>
                <div class="revision-time">{formatTime(props.message.editedAtMs!)}</div>
              </Show>
            </div>
          </Show>
        </div>
      </div>
    </div>
  );
}
