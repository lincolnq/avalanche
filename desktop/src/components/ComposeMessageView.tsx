import { createSignal, createEffect, on, onMount, onCleanup, For, Show } from "solid-js";
import { FiChevronUp, FiChevronDown, FiArrowUp, FiX } from "solid-icons/fi";
import { TbOutlinePaperclip, TbOutlineFile } from "solid-icons/tb";
import { useApp } from "../state/AppContext";
import type { Conversation, Message } from "../models";
import type { AttachmentFfi, LinkPreviewFfi } from "../bindings";
import { firstUrl } from "../lib/format";
import { makeImageThumbnail } from "../lib/image";
import LinkPreviewCard from "./LinkPreviewCard";
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
/** Link-preview fetch debounce (matches iOS 600ms). */
const PREVIEW_DEBOUNCE_MS = 600;

export default function ComposeMessageView(props: Props) {
  const {
    sendMessage,
    sendGroupMessage,
    editMessage,
    sendMessageWithAttachments,
    uploadAttachment,
    fetchLinkPreview,
  } = useApp();
  const [draft, setDraft] = createSignal("");
  const [sending, setSending] = createSignal(false);
  const [uploading, setUploading] = createSignal(false);
  const [expanded, setExpanded] = createSignal(false);
  const [mounted, setMounted] = createSignal(false);

  // Staged attachments (uploaded pointers awaiting send) and their preview blob
  // URLs (for the chip thumbnails), kept in lockstep by index.
  const [stagedAttachments, setStagedAttachments] = createSignal<AttachmentFfi[]>([]);
  const [stagedPreview, setStagedPreview] = createSignal<LinkPreviewFfi | null>(null);
  // Tracked outside reactive state — staging dedupe, exactly like iOS.
  let stagedPreviewUrl: string | null = null;
  let dismissedPreviewUrl: string | null = null;
  let previewTimer: number | undefined;

  let inputRef: HTMLTextAreaElement | undefined;
  let fileInputRef: HTMLInputElement | undefined;

  onMount(() => {
    setMounted(true);
    inputRef?.focus();
  });

  onCleanup(() => {
    if (previewTimer) clearTimeout(previewTimer);
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

  // Reset the composer when switching conversations. ChatsView's <Show> is not
  // keyed, so this ComposeMessageView instance is reused across switches — without
  // this, a staged attachment or draft would carry over and could be sent to the
  // wrong recipient. `defer` skips the initial mount. (iOS gets this for free via
  // a fresh ConversationView per conversation.)
  createEffect(
    on(
      () => props.conversation.id,
      () => {
        setDraft("");
        clearStaging();
        setTimeout(() => resizeTextarea(), 0);
      },
      { defer: true }
    )
  );

  // Debounced link-preview detection. Tracks the draft only; staging state is
  // managed through plain locals + setters to avoid re-entrant effect loops.
  // Mirrors iOS schedulePreviewFetch: skip the already-staged or dismissed URL,
  // and reset the dismissal once the URL leaves the text.
  createEffect(() => {
    const text = draft();
    if (previewTimer) clearTimeout(previewTimer);
    if (props.editingMessage) return; // edits don't carry previews
    const url = firstUrl(text);
    if (!url) {
      dismissedPreviewUrl = null;
      stagedPreviewUrl = null;
      setStagedPreview(null);
      return;
    }
    // Re-enable previews once the first URL differs from the dismissed one, so
    // re-typing a previously-dismissed URL fetches again (iOS parity).
    if (url !== dismissedPreviewUrl) dismissedPreviewUrl = null;
    if (url === stagedPreviewUrl || url === dismissedPreviewUrl) return;
    previewTimer = window.setTimeout(() => void loadPreview(url), PREVIEW_DEBOUNCE_MS);
  });

  async function loadPreview(url: string) {
    if (url === dismissedPreviewUrl) return;
    if (firstUrl(draft()) !== url) return;
    try {
      const meta = await fetchLinkPreview(url);
      // Nothing worth showing — skip (iOS only stages cards with content).
      if (!meta.title && meta.imageBytes.length === 0) return;
      let image: AttachmentFfi | null = null;
      if (meta.imageBytes.length > 0) {
        image = await uploadAttachment(
          props.conversation.accountId,
          meta.imageBytes,
          meta.imageContentType ?? "image/jpeg",
          null,
          0,
          0,
          0,
          [],
          0
        );
      }
      // The draft may have changed during the async fetch/upload.
      if (firstUrl(draft()) !== url || url === dismissedPreviewUrl) return;
      stagedPreviewUrl = url;
      setStagedPreview({
        // Use the body URL (not meta.url, which may be a redirect/canonical
        // form): LinkPreviewFfi.url must occur verbatim in the body or the
        // recipient's anti-spoof filter drops the card. Matches iOS.
        url,
        title: meta.title,
        description: meta.description,
        dateMs: meta.dateMs,
        image,
      });
    } catch (err) {
      console.warn("fetchLinkPreview failed:", err);
    }
  }

  function dismissPreview() {
    dismissedPreviewUrl = stagedPreviewUrl;
    stagedPreviewUrl = null;
    setStagedPreview(null);
  }

  function clearStaging() {
    if (previewTimer) clearTimeout(previewTimer);
    stagedPreviewUrl = null;
    dismissedPreviewUrl = null;
    setStagedPreview(null);
    setStagedAttachments([]);
  }

  async function onFilePicked(e: Event & { currentTarget: HTMLInputElement }) {
    const file = e.currentTarget.files?.[0];
    e.currentTarget.value = ""; // allow re-picking the same file
    if (!file) return;
    setUploading(true);
    try {
      const buf = new Uint8Array(await file.arrayBuffer());
      const contentType = file.type || "application/octet-stream";
      let thumbnail: number[] = [];
      let width = 0;
      let height = 0;
      if (contentType.startsWith("image/")) {
        try {
          const t = await makeImageThumbnail(file);
          thumbnail = t.thumbnail;
          width = t.width;
          height = t.height;
        } catch (err) {
          console.warn("thumbnail generation failed:", err);
        }
      }
      const pointer = await uploadAttachment(
        props.conversation.accountId,
        Array.from(buf),
        contentType,
        file.name,
        width,
        height,
        0,
        thumbnail,
        0
      );
      // Keep the locally-computed thumbnail on the staged pointer so the chip
      // and the optimistic bubble render instantly without a round-trip.
      setStagedAttachments((prev) => [...prev, { ...pointer, thumbnail }]);
    } catch (err) {
      console.warn("attachment upload failed:", err);
    } finally {
      setUploading(false);
    }
  }

  function removeStagedAttachment(index: number) {
    setStagedAttachments((prev) => prev.filter((_, i) => i !== index));
  }

  function resizeTextarea() {
    const el = inputRef;
    if (!el) return;
    el.style.height = "auto";
    const h = expanded()
      ? Math.min(Math.max(el.scrollHeight, 120), EXPANDED_MAX)
      : Math.min(el.scrollHeight, COLLAPSED_MAX);
    el.style.height = `${h}px`;
  }

  function toggleExpand() {
    setExpanded((prev) => !prev);
    setTimeout(() => resizeTextarea(), 0);
  }

  function canSend(): boolean {
    return (
      !!draft().trim() || stagedAttachments().length > 0 || stagedPreview() !== null
    );
  }

  async function handleSend() {
    if (sending() || uploading()) return;
    const text = draft().trim();

    // Edit mode: apply the edit (optimistic + async FFI) and exit. Edits never
    // carry attachments or previews.
    const editing = props.editingMessage;
    if (editing) {
      if (!text) return;
      editMessage(props.conversation, editing, text);
      setDraft("");
      props.onCancelEdit?.();
      setExpanded(false);
      setTimeout(() => resizeTextarea(), 0);
      return;
    }

    const attachments = stagedAttachments();
    const preview = stagedPreview();
    const hasExtras = attachments.length > 0 || preview !== null;
    if (!text && !hasExtras) return;
    if (!props.conversation.isGroup && !props.conversation.recipientDid) return;

    setDraft("");
    const previews = preview ? [preview] : [];
    clearStaging();
    setSending(true);
    setExpanded(false);
    setTimeout(() => resizeTextarea(), 0);
    try {
      if (hasExtras) {
        await sendMessageWithAttachments(props.conversation, text, attachments, previews);
      } else if (props.conversation.isGroup) {
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

      <Show when={stagedAttachments().length > 0 || stagedPreview()}>
        <div class="compose-staging">
          <For each={stagedAttachments()}>
            {(att, i) => <StagedAttachmentChip attachment={att} onRemove={() => removeStagedAttachment(i())} />}
          </For>
          <Show when={stagedPreview()}>
            {(p) => (
              <LinkPreviewCard
                preview={p()}
                accountId={props.conversation.accountId}
                onDismiss={dismissPreview}
              />
            )}
          </Show>
        </div>
      </Show>

      <div class="compose-row">
        <Show when={!props.editingMessage}>
          <button
            class="compose-attach-btn"
            aria-label="Attach a file"
            disabled={sending() || uploading()}
            onClick={() => fileInputRef?.click()}
          >
            <TbOutlinePaperclip size={20} />
          </button>
          <input
            ref={fileInputRef}
            class="compose-file-input"
            type="file"
            accept="image/*"
            onChange={onFilePicked}
          />
        </Show>
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
        <button class="send-btn" disabled={!canSend() || sending() || uploading()} onClick={handleSend}>
          <FiArrowUp size={20} />
        </button>
      </div>
    </div>
  );
}

/** A staged (not-yet-sent) attachment: image thumbnail or file name, with a ×. */
function StagedAttachmentChip(props: { attachment: AttachmentFfi; onRemove: () => void }) {
  const isImage = () => props.attachment.contentType.startsWith("image/");
  const [url, setUrl] = createSignal<string | null>(null);

  onMount(() => {
    const thumb = props.attachment.thumbnail;
    if (isImage() && thumb.length > 0) {
      setUrl(URL.createObjectURL(new Blob([new Uint8Array(thumb)], { type: "image/jpeg" })));
    }
  });
  onCleanup(() => {
    const u = url();
    if (u) URL.revokeObjectURL(u);
  });

  return (
    <div class="staged-chip">
      <Show
        when={isImage() && url()}
        fallback={
          <span class="staged-chip-file">
            <TbOutlineFile size={16} />
            <span class="staged-chip-name">{props.attachment.fileName ?? "Attachment"}</span>
          </span>
        }
      >
        <img class="staged-chip-image" src={url()!} alt="" />
      </Show>
      <button class="staged-chip-remove" aria-label="Remove attachment" onClick={props.onRemove}>
        ×
      </button>
    </div>
  );
}
