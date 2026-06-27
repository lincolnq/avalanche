import { createSignal, createEffect, onMount, Show } from "solid-js";
import { FiChevronUp, FiChevronDown, FiArrowUp, FiX } from "solid-icons/fi";
import { useApp } from "../state/AppContext";
import type { Conversation, Message } from "../models";
import "./ComposeMessageView.css";

interface Props {
  conversation: Conversation;
  editingMessage?: Message | null;
  onCancelEdit?: () => void;
}

/** Collapsed max-height (~2-3 lines). */
const COLLAPSED_MAX = 72;
/** Expanded max-height for long messages. */
const EXPANDED_MAX = 212;

export default function ComposeMessageView(props: Props) {
  const { sendMessage, sendGroupMessage, editMessage } = useApp();
  const [draft, setDraft] = createSignal("");
  const [sending, setSending] = createSignal(false);
  const [expanded, setExpanded] = createSignal(false);
  const [mounted, setMounted] = createSignal(false);
  let inputRef: HTMLTextAreaElement | undefined;

  onMount(() => {
    // Reveal the caret only after the editor has fully mounted, so there's
    // no flash of a blinking cursor before the rich-text engine initialises.
    setMounted(true);
    inputRef?.focus();
  });

  // Entering/leaving edit mode pre-fills (or clears) the draft. Tracks only
  // props.editingMessage, so normal typing never re-triggers this.
  createEffect(() => {
    const editing = props.editingMessage;
    if (editing) {
      setDraft(editing.body);
      setTimeout(() => {
        inputRef?.focus();
        resizeTextarea();
      }, 0);
    } else {
      setDraft("");
      setTimeout(() => resizeTextarea(), 0);
    }
  });

  function resizeTextarea() {
    const el = inputRef;
    if (!el) return;
    el.style.height = "auto";
    // When expanded, the textarea has a minimum visible height so the
    // toggle is a visible change even when the box is empty.  Collapsed
    // mode only grows to fit content up to the collapsed cap.
    const h = expanded()
      ? Math.min(Math.max(el.scrollHeight, 120), EXPANDED_MAX)
      : Math.min(el.scrollHeight, COLLAPSED_MAX);
    el.style.height = `${h}px`;
  }

  function toggleExpand() {
    setExpanded((prev) => !prev);
    // Defer so the signal propagates before reading clientHeight.
    setTimeout(() => resizeTextarea(), 0);
  }

  async function handleSend() {
    const text = draft().trim();
    if (!text || sending()) return;

    // Edit mode: apply the edit (optimistic + async FFI) and exit.
    const editing = props.editingMessage;
    if (editing) {
      editMessage(props.conversation, editing, text);
      setDraft("");
      props.onCancelEdit?.();
      setExpanded(false);
      setTimeout(() => resizeTextarea(), 0);
      return;
    }

    if (!props.conversation.isGroup && !props.conversation.recipientDid) return;

    setDraft("");
    setSending(true);
    setExpanded(false);
    setTimeout(() => resizeTextarea(), 0);
    try {
      if (props.conversation.isGroup) {
        await sendGroupMessage(props.conversation, text);
      } else {
        await sendMessage(
          props.conversation.id,
          text,
          props.conversation.recipientDid!,
          props.conversation.accountId
        );
      }
    } catch {
      // optimistic update already shows failed state
    } finally {
      setSending(false);
    }
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void handleSend();
    } else if (e.key === "Escape" && props.editingMessage) {
      e.preventDefault();
      props.onCancelEdit?.();
    }
  }

  return (
    <div class="compose-row-wrap">
      <Show when={props.editingMessage}>
        <div class="compose-editing-bar">
          <span>Editing message</span>
          <button
            class="compose-editing-cancel"
            onClick={() => props.onCancelEdit?.()}
            aria-label="Cancel edit"
          >
            <FiX size={14} />
            Cancel
          </button>
        </div>
      </Show>
      <div class="compose-row">
        <div class="compose-input-wrap" classList={{ expanded: expanded() }}>
          <textarea
            ref={inputRef}
            class="compose-input scrollbar-thin"
            classList={{ mounted: mounted(), expanded: expanded() }}
            placeholder="Message"
            rows={1}
            value={draft()}
            onInput={(e) => {
              setDraft(e.currentTarget.value);
              resizeTextarea();
            }}
            onKeyDown={handleKeyDown}
            disabled={sending()}
          />
          {!sending() && (
            <button
              class="compose-expand-tab"
              onClick={toggleExpand}
              aria-label={expanded() ? "Collapse" : "Expand"}
            >
              {expanded() ? <FiChevronDown size={14} /> : <FiChevronUp size={14} />}
            </button>
          )}
        </div>
        <button
          class="send-btn"
          disabled={!draft().trim() || sending()}
          onClick={handleSend}
        >
          <FiArrowUp size={20} />
        </button>
      </div>
    </div>
  );
}
