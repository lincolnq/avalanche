import { createSignal, createEffect, onMount, onCleanup, Show } from "solid-js";
import { TbOutlineX, TbOutlineChevronLeft, TbOutlineChevronRight } from "solid-icons/tb";
import { useApp } from "../state/AppContext";
import type { AttachmentFfi } from "../bindings";
import "./ImageViewerModal.css";

interface Props {
  // All image attachments in the conversation, in timeline order.
  images: AttachmentFfi[];
  // The attachment id the user clicked — where the viewer opens.
  startId: string;
  accountId: string;
  onClose: () => void;
}

const MAX_SCALE = 6;

/**
 * Fullscreen image viewer (docs/35): the desktop analogue of the mobile viewer.
 * Paging (mobile swipe) is arrow buttons + Left/Right keys; zoom (mobile pinch)
 * is the mouse wheel / trackpad pinch + double-click; pan is click-drag when
 * zoomed; dismiss (mobile swipe-down) is Esc, the close button, or a backdrop
 * click. Pages through every image in the conversation in timeline order.
 *
 * The image `transform` is applied imperatively via a ref (CSSOM) rather than an
 * inline style attribute, which the strict production CSP forbids — matching the
 * `ComposeMessageView` textarea-autosize pattern.
 */
export default function ImageViewerModal(props: Props) {
  const app = useApp();
  const startIndex = Math.max(0, props.images.findIndex((a) => a.id === props.startId));
  const [index, setIndex] = createSignal(startIndex);
  const [url, setUrl] = createSignal<string | null>(null);
  const [scale, setScale] = createSignal(1);
  const [tx, setTx] = createSignal(0);
  const [ty, setTy] = createSignal(0);

  // Full-resolution blob URLs cached by attachment id; revoked on cleanup.
  const urlCache = new Map<string, string>();
  const created: string[] = [];
  let disposed = false;
  let imgEl: HTMLImageElement | undefined;

  const current = () => props.images[index()];

  async function loadImage(att: AttachmentFfi) {
    const cached = urlCache.get(att.id);
    if (cached) {
      setUrl(cached);
      return;
    }
    setUrl(null);
    try {
      const bytes = await app.downloadAttachment(props.accountId, att);
      if (disposed || bytes.length === 0) return;
      const objectUrl = URL.createObjectURL(
        new Blob([new Uint8Array(bytes)], { type: att.contentType }),
      );
      created.push(objectUrl);
      urlCache.set(att.id, objectUrl);
      // Only show it if this is still the current image (user may have paged on).
      if (att.id === current().id) setUrl(objectUrl);
    } catch (e) {
      console.warn("image viewer download failed:", e);
    }
  }

  function resetZoom() {
    setScale(1);
    setTx(0);
    setTy(0);
  }

  function go(delta: number) {
    const n = props.images.length;
    if (n === 0) return;
    setIndex((index() + delta + n) % n);
    resetZoom();
  }
  const prev = () => go(-1);
  const next = () => go(1);

  // Load the current image (and reset zoom) whenever the index changes.
  createEffect(() => {
    const att = current();
    if (att) void loadImage(att);
  });

  // Apply zoom/pan to the <img> via CSSOM (no inline style attribute; CSP-safe).
  createEffect(() => {
    if (imgEl) imgEl.style.transform = `translate(${tx()}px, ${ty()}px) scale(${scale()})`;
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") props.onClose();
    else if (e.key === "ArrowLeft") prev();
    else if (e.key === "ArrowRight") next();
  }
  onMount(() => window.addEventListener("keydown", onKey));
  onCleanup(() => {
    disposed = true;
    window.removeEventListener("keydown", onKey);
    for (const u of created) URL.revokeObjectURL(u);
  });

  // Wheel / trackpad-pinch (Ctrl+wheel) zooms.
  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const factor = e.deltaY < 0 ? 1.15 : 1 / 1.15;
    const nextScale = Math.min(MAX_SCALE, Math.max(1, scale() * factor));
    if (nextScale <= 1) resetZoom();
    else setScale(nextScale);
  }

  function onDblClick() {
    if (scale() > 1) resetZoom();
    else setScale(2.5);
  }

  // Click-drag to pan when zoomed.
  let dragging = false;
  let lastX = 0;
  let lastY = 0;
  function onPointerDown(e: PointerEvent) {
    if (scale() <= 1) return;
    dragging = true;
    lastX = e.clientX;
    lastY = e.clientY;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function onPointerMove(e: PointerEvent) {
    if (!dragging) return;
    setTx(tx() + (e.clientX - lastX));
    setTy(ty() + (e.clientY - lastY));
    lastX = e.clientX;
    lastY = e.clientY;
  }
  function onPointerUp() {
    dragging = false;
  }

  // A backdrop click dismisses only when not zoomed (so a pan-release doesn't).
  function onBackdropClick() {
    if (scale() <= 1) props.onClose();
  }

  return (
    <div class="viewer-backdrop" onClick={onBackdropClick}>
      <button class="viewer-close" onClick={props.onClose} aria-label="Close">
        <TbOutlineX size={22} />
      </button>
      <Show when={props.images.length > 1}>
        <button
          class="viewer-nav viewer-prev"
          aria-label="Previous image"
          onClick={(e) => {
            e.stopPropagation();
            prev();
          }}
        >
          <TbOutlineChevronLeft size={32} />
        </button>
        <button
          class="viewer-nav viewer-next"
          aria-label="Next image"
          onClick={(e) => {
            e.stopPropagation();
            next();
          }}
        >
          <TbOutlineChevronRight size={32} />
        </button>
      </Show>
      <div
        class="viewer-stage"
        onClick={(e) => e.stopPropagation()}
        onWheel={onWheel}
        onDblClick={onDblClick}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
      >
        <Show when={url()} fallback={<div class="viewer-spinner" />}>
          <img
            ref={imgEl}
            class="viewer-img"
            classList={{ "viewer-img-zoomed": scale() > 1 }}
            src={url()!}
            alt={current()?.fileName ?? "Image"}
            draggable={false}
          />
        </Show>
      </div>
    </div>
  );
}
