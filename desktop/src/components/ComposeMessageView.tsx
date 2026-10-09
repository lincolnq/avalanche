import { createSignal, createEffect, on, onMount, onCleanup, For, Show, type Accessor } from "solid-js";
import { FiChevronUp, FiChevronDown, FiArrowUp, FiX } from "solid-icons/fi";
import { TbOutlinePaperclip, TbOutlineFile, TbOutlineUserPlus } from "solid-icons/tb";
import { useApp } from "../state/AppContext";
import type { Conversation, Message } from "../models";
import type { AttachmentFfi, LinkPreviewFfi, SharedContactFfi } from "../bindings";
import { firstUrl } from "../lib/format";
import { prepareImageForSending } from "../lib/image";
import { copiedContact } from "../lib/contactClipboard";
import LinkPreviewCard from "./LinkPreviewCard";
import SharedContactCard from "./SharedContactCard";
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

/**
 * A staged image. The chip shows as soon as the image is decoded; the upload
 * runs in the background and `ready` flips when it lands. Send awaits any
 * still-pending `upload`.
 */
interface StagedImage {
  key: number;
  contentType: string;
  fileName: string | null;
  /** Local JPEG thumbnail for the chip (and the optimistic bubble). */
  thumbnail: number[];
  ready: Accessor<boolean>;
  /** Resolves to the uploaded pointer, or null if the upload failed. */
  upload: Promise<AttachmentFfi | null>;
}

/** Image files on a clipboard/drop, from `files` or (WebKit fallback) `items`. */
function imageFiles(data: DataTransfer | null | undefined): File[] {
  if (!data) return [];
  let files = Array.from(data.files);
  if (files.length === 0) {
    files = Array.from(data.items)
      .filter((it) => it.kind === "file")
      .map((it) => it.getAsFile())
      .filter((f): f is File => f !== null);
  }
  return files.filter((f) => f.type.startsWith("image/"));
}

/** Swap a re-encoded image's extension to match its new type ("a.png" -> "a.jpg"). */
function renameForType(name: string, contentType: string): string {
  if (contentType !== "image/jpeg") return name;
  const dot = name.lastIndexOf(".");
  return (dot > 0 ? name.slice(0, dot) : name || "image") + ".jpg";
}

/** True for a text field other than the composer, where paste should stay put. */
function isOtherEditable(target: EventTarget | null, composer: HTMLElement | undefined): boolean {
  if (!(target instanceof HTMLElement) || target === composer) return false;
  return (
    target.isContentEditable ||
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement
  );
}

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
  const [expanded, setExpanded] = createSignal(false);
  const [mounted, setMounted] = createSignal(false);

  // Staged images awaiting send (uploading or uploaded).
  const [stagedAttachments, setStagedAttachments] = createSignal<StagedImage[]>([]);
  let nextStagedKey = 0;
  const [stagedPreview, setStagedPreview] = createSignal<LinkPreviewFfi | null>(null);
  // A staged shared contact card (docs/35), pasted from a "Copy contact" action,
  // shown as a chip until you send or remove it.
  const [stagedContact, setStagedContact] = createSignal<SharedContactFfi | null>(null);
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
    setStagedContact(null);
  }

  async function onFilePicked(e: Event & { currentTarget: HTMLInputElement }) {
    const file = e.currentTarget.files?.[0];
    e.currentTarget.value = ""; // allow re-picking the same file
    if (!file) return;
    await stageImages([file]);
  }

  // Pasted or dropped images stage exactly like a picked file. Only images, to
  // match the picker (accept="image/*"); anything else is ignored.
  async function stageImages(files: File[]) {
    for (const f of files.filter((f) => f.type.startsWith("image/"))) {
      await stageFile(f);
    }
  }

  // Paste anywhere in the window while a conversation is open (not just with the
  // composer focused) stages clipboard images. Pastes into other text fields are
  // left alone, and plain-text pastes fall through to the textarea.
  function onPaste(e: ClipboardEvent) {
    if (e.defaultPrevented || props.editingMessage) return;
    if (isOtherEditable(e.target, inputRef)) return;
    const files = imageFiles(e.clipboardData);
    if (files.length === 0) return;
    e.preventDefault();
    void stageImages(files);
    inputRef?.focus();
  }

  // Drag-and-drop anywhere over the window while a conversation's composer is
  // mounted (requires the main window's dragDropEnabled: false, so the webview
  // receives HTML5 drop events instead of Tauri's native handler).
  const [dragging, setDragging] = createSignal(false);
  let dragDepth = 0;
  const hasFiles = (e: DragEvent) => Array.from(e.dataTransfer?.types ?? []).includes("Files");
  function onDragEnter(e: DragEvent) {
    if (!hasFiles(e) || props.editingMessage) return;
    dragDepth++;
    setDragging(true);
  }
  function onDragLeave(e: DragEvent) {
    if (!hasFiles(e)) return;
    dragDepth = Math.max(0, dragDepth - 1);
    if (dragDepth === 0) setDragging(false);
  }
  function onDragOver(e: DragEvent) {
    if (!hasFiles(e) || props.editingMessage) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
  }
  function onDrop(e: DragEvent) {
    dragDepth = 0;
    setDragging(false);
    if (!hasFiles(e) || props.editingMessage) return;
    e.preventDefault();
    void stageImages(imageFiles(e.dataTransfer));
    inputRef?.focus();
  }
  onMount(() => {
    window.addEventListener("paste", onPaste);
    window.addEventListener("dragenter", onDragEnter);
    window.addEventListener("dragleave", onDragLeave);
    window.addEventListener("dragover", onDragOver);
    window.addEventListener("drop", onDrop);
  });
  onCleanup(() => {
    window.removeEventListener("paste", onPaste);
    window.removeEventListener("dragenter", onDragEnter);
    window.removeEventListener("dragleave", onDragLeave);
    window.removeEventListener("dragover", onDragOver);
    window.removeEventListener("drop", onDrop);
  });

  // Prepare the image (downscale + JPEG, docs/35), show its chip right away, and
  // upload in the background. Clearing staging (send, conversation switch) or
  // removing the chip drops the entry, so a late upload result is ignored.
  async function stageFile(file: File) {
    let prepared;
    try {
      prepared = await prepareImageForSending(file);
    } catch (err) {
      console.warn("image decode failed:", err);
      return;
    }
    const key = nextStagedKey++;
    const { thumbnail, contentType, width, height } = prepared;
    const fileName = renameForType(file.name, contentType);
    const upload = uploadAttachment(
      props.conversation.accountId,
      Array.from(prepared.bytes),
      contentType,
      fileName,
      width,
      height,
      0,
      thumbnail,
      0
    )
      // Keep the locally-computed thumbnail on the pointer so the optimistic
      // bubble renders instantly without a round-trip.
      .then((pointer): AttachmentFfi => ({ ...pointer, thumbnail }))
      .catch((err) => {
        console.warn("attachment upload failed:", err);
        return null;
      });
    const [ready, setReady] = createSignal(false);
    setStagedAttachments((prev) => [...prev, { key, contentType, fileName, thumbnail, ready, upload }]);
    if (await upload) setReady(true);
    else removeStagedAttachment(key);
  }

  function removeStagedAttachment(key: number) {
    setStagedAttachments((prev) => prev.filter((s) => s.key !== key));
  }

  function resizeTextarea() {
    const el = inputRef;
    if (!el) return;
    el.style.height = "auto";
    // scrollHeight covers content + padding but NOT the border. With the global
    // border-box sizing (theme.css), the box height must add the border back, or
    // the content is clipped 1px at top and bottom and a scrollbar shows even at
    // a single line. offsetHeight - clientHeight is the vertical border (2px).
    const border = el.offsetHeight - el.clientHeight;
    const content = el.scrollHeight + border;
    const h = expanded()
      ? Math.min(Math.max(content, 120), EXPANDED_MAX)
      : Math.min(content, COLLAPSED_MAX);
    el.style.height = `${h}px`;
  }

  function toggleExpand() {
    setExpanded((prev) => !prev);
    setTimeout(() => resizeTextarea(), 0);
  }

  function canSend(): boolean {
    return (
      !!draft().trim() ||
      stagedAttachments().length > 0 ||
      stagedPreview() !== null ||
      stagedContact() !== null
    );
  }

  async function handleSend() {
    if (sending()) return;
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

    const staged = stagedAttachments();
    const preview = stagedPreview();
    const contact = stagedContact();
    if (!text && staged.length === 0 && !preview && !contact) return;
    if (!props.conversation.isGroup && !props.conversation.recipientDid) return;

    setDraft("");
    const previews = preview ? [preview] : [];
    const contacts = contact ? [contact] : [];
    clearStaging();
    setSending(true);
    setExpanded(false);
    setTimeout(() => resizeTextarea(), 0);
    try {
      // Wait out any upload still in flight; drop ones that failed.
      const attachments = (await Promise.all(staged.map((s) => s.upload))).filter(
        (p): p is AttachmentFfi => p !== null
      );
      const hasExtras = attachments.length > 0 || previews.length > 0 || contacts.length > 0;
      if (!text && !hasExtras) return;
      if (hasExtras) {
        await sendMessageWithAttachments(props.conversation, text, attachments, previews, contacts);
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
      <Show when={dragging()}>
        <div class="compose-drop-overlay">
          <div class="compose-drop-card">Drop images to attach</div>
        </div>
      </Show>
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

      <Show when={stagedAttachments().length > 0 || stagedPreview() || stagedContact()}>
        <div class="compose-staging">
          <For each={stagedAttachments()}>
            {(img) => <StagedAttachmentChip image={img} onRemove={() => removeStagedAttachment(img.key)} />}
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
          <Show when={stagedContact()}>
            {(c) => (
              <SharedContactCard
                contact={c()}
                accountId={props.conversation.accountId}
                mine={true}
                staged={true}
                onDismiss={() => setStagedContact(null)}
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
            disabled={sending()}
            onClick={() => fileInputRef?.click()}
          >
            <TbOutlinePaperclip size={24} />
          </button>
          <input
            ref={fileInputRef}
            class="compose-file-input"
            type="file"
            accept="image/*"
            onChange={onFilePicked}
          />
          {/* Paste a copied contact card (docs/35) — shown when the in-app
              contact clipboard holds one and none is staged yet. */}
          <Show when={copiedContact() && !stagedContact()}>
            <button
              class="compose-attach-btn"
              aria-label="Paste contact"
              title="Paste contact"
              disabled={sending()}
              onClick={() => setStagedContact(copiedContact())}
            >
              <TbOutlineUserPlus size={24} />
            </button>
          </Show>
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
        <button class="send-btn" disabled={!canSend() || sending()} onClick={handleSend}>
          <FiArrowUp size={24} />
        </button>
      </div>
    </div>
  );
}

/**
 * A staged (not-yet-sent) image: its thumbnail (or file name), with a spinner
 * while the upload is in flight, and a ×.
 */
function StagedAttachmentChip(props: { image: StagedImage; onRemove: () => void }) {
  const isImage = () => props.image.contentType.startsWith("image/");
  const [url, setUrl] = createSignal<string | null>(null);

  onMount(() => {
    const thumb = props.image.thumbnail;
    if (isImage() && thumb.length > 0) {
      setUrl(URL.createObjectURL(new Blob([new Uint8Array(thumb)], { type: "image/jpeg" })));
    }
  });
  onCleanup(() => {
    const u = url();
    if (u) URL.revokeObjectURL(u);
  });

  return (
    <div class="staged-chip" classList={{ uploading: !props.image.ready() }}>
      <Show
        when={isImage() && url()}
        fallback={
          <span class="staged-chip-file">
            <TbOutlineFile size={16} />
            <span class="staged-chip-name">{props.image.fileName ?? "Attachment"}</span>
          </span>
        }
      >
        <img class="staged-chip-image" src={url()!} alt="" />
      </Show>
      <Show when={!props.image.ready()}>
        <div class="staged-chip-progress" aria-label="Uploading">
          <div class="spinner" />
        </div>
      </Show>
      <button class="staged-chip-remove" aria-label="Remove attachment" onClick={props.onRemove}>
        ×
      </button>
    </div>
  );
}
