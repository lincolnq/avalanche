import { createSignal, onMount, onCleanup, Show } from "solid-js";
import { TbOutlineFile } from "solid-icons/tb";
import { useApp } from "../state/AppContext";
import type { AttachmentFfi } from "../bindings";
import { formatBytes } from "../lib/format";
import "./AttachmentView.css";

interface Props {
  attachment: AttachmentFfi;
  // The owning conversation's account — attachments decrypt with its keys.
  accountId: string;
}

function blobUrl(bytes: number[], contentType: string): string {
  return URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: contentType }));
}

/**
 * Renders one attachment (docs/35). Images show the inline thumbnail first
 * (blurred placeholder) then swap to the full blob once `download_attachment`
 * returns; non-image attachments render as a tappable file chip that saves the
 * decrypted blob. Blob URLs are cached on the instance and revoked on cleanup.
 * Mirrors iOS `AttachmentView`.
 */
export default function AttachmentView(props: Props) {
  const app = useApp();
  const isImage = () => props.attachment.contentType.startsWith("image/");

  const [thumbUrl, setThumbUrl] = createSignal<string | null>(null);
  const [fullUrl, setFullUrl] = createSignal<string | null>(null);
  const [saving, setSaving] = createSignal(false);
  const created: string[] = [];
  // The async download can resolve after the component unmounts; revoke any
  // blob URL created post-cleanup immediately rather than leaking it.
  let disposed = false;

  function track(url: string): string {
    if (disposed) {
      URL.revokeObjectURL(url);
      return url;
    }
    created.push(url);
    return url;
  }

  onMount(() => {
    // Inline thumbnail (downscaled JPEG) renders instantly as a placeholder.
    const thumb = props.attachment.thumbnail;
    if (isImage() && thumb.length > 0) {
      setThumbUrl(track(blobUrl(thumb, "image/jpeg")));
    }
    if (isImage()) {
      void app
        .downloadAttachment(props.accountId, props.attachment)
        .then((bytes) => {
          if (disposed || bytes.length === 0) return;
          setFullUrl(track(blobUrl(bytes, props.attachment.contentType)));
        })
        .catch((e: unknown) => {
          console.warn("downloadAttachment failed:", e);
        });
    }
  });

  onCleanup(() => {
    disposed = true;
    for (const url of created) URL.revokeObjectURL(url);
  });

  // Save a non-image attachment's decrypted bytes via the browser download path.
  async function saveFile() {
    if (saving()) return;
    setSaving(true);
    try {
      const bytes = await app.downloadAttachment(props.accountId, props.attachment);
      const url = blobUrl(bytes, props.attachment.contentType);
      const a = document.createElement("a");
      a.href = url;
      a.download = props.attachment.fileName ?? "attachment";
      a.click();
      // Revoke after the click has had a chance to start the download.
      setTimeout(() => URL.revokeObjectURL(url), 10_000);
    } catch (e) {
      console.warn("saveFile failed:", e);
    } finally {
      setSaving(false);
    }
  }

  return (
    <Show
      when={isImage()}
      fallback={
        <button class="attachment-file" onClick={saveFile} disabled={saving()}>
          <TbOutlineFile size={20} />
          <span class="attachment-file-meta">
            <span class="attachment-file-name">
              {props.attachment.fileName ?? "Attachment"}
            </span>
            <span class="attachment-file-size">
              {formatBytes(props.attachment.sizeBytes)}
            </span>
          </span>
        </button>
      }
    >
      <img
        class="attachment-image"
        classList={{ "attachment-blurred": fullUrl() === null && thumbUrl() !== null }}
        src={fullUrl() ?? thumbUrl() ?? ""}
        alt={props.attachment.fileName ?? "Image attachment"}
      />
    </Show>
  );
}
