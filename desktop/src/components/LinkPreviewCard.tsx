import { createSignal, onMount, onCleanup, Show } from "solid-js";
import { useApp } from "../state/AppContext";
import type { LinkPreviewFfi } from "../bindings";
import { displayHost } from "../lib/format";
import "./LinkPreviewCard.css";

interface Props {
  preview: LinkPreviewFfi;
  // The owning conversation's account — the og:image decrypts with its keys.
  accountId: string;
  /** Compose-staging variant shows a dismiss (×) affordance via `onDismiss`. */
  onDismiss?: () => void;
}

/**
 * A rich link-preview card (docs/35): og:image, title, and source domain. The
 * whole card opens the URL in the OS browser (never in-app). Mirrors iOS
 * `LinkPreviewCard`. The og:image rides the encrypted attachment path, so it is
 * downloaded like any other blob.
 */
export default function LinkPreviewCard(props: Props) {
  const app = useApp();
  const [imageUrl, setImageUrl] = createSignal<string | null>(null);
  let createdUrl: string | null = null;
  // The async download can resolve after unmount; drop a post-cleanup blob URL.
  let disposed = false;

  const domain = () => displayHost(props.preview.url, props.preview.url).replace(/^www\./, "");

  onMount(() => {
    const image = props.preview.image;
    if (image) {
      void app
        .downloadAttachment(props.accountId, image)
        .then((bytes) => {
          if (bytes.length === 0) return;
          const url = URL.createObjectURL(
            new Blob([new Uint8Array(bytes)], { type: image.contentType })
          );
          if (disposed) {
            URL.revokeObjectURL(url);
            return;
          }
          createdUrl = url;
          setImageUrl(url);
        })
        .catch((e: unknown) => console.warn("link preview image failed:", e));
    }
  });

  onCleanup(() => {
    disposed = true;
    if (createdUrl) URL.revokeObjectURL(createdUrl);
  });

  return (
    <div class="link-preview-card">
      <button
        class="link-preview-open"
        onClick={() => void app.openExternal(props.preview.url)}
      >
        <Show when={imageUrl()}>
          <img class="link-preview-image" src={imageUrl()!} alt="" />
        </Show>
        <div class="link-preview-text">
          <Show when={props.preview.title}>
            <span class="link-preview-title">{props.preview.title}</span>
          </Show>
          <span class="link-preview-domain">{domain()}</span>
        </div>
      </button>
      <Show when={props.onDismiss}>
        <button class="link-preview-dismiss" aria-label="Remove preview" onClick={props.onDismiss}>
          ×
        </button>
      </Show>
    </div>
  );
}
