/** Outgoing-image caps, matching iOS `OutgoingImage` (docs/35). */
const OUTGOING_MAX_DIMENSION = 2048;
const OUTGOING_JPEG_QUALITY = 0.9;

export interface PreparedImage {
  /** Bytes to upload: downscaled, re-encoded JPEG (or the original GIF). */
  bytes: Uint8Array;
  contentType: string;
  /** Dimensions of `bytes`. */
  width: number;
  height: number;
  /** Small JPEG for the chip and the inline bubble preview. */
  thumbnail: number[];
}

/**
 * Prepare a picked/pasted/dropped image for sending, mirroring iOS
 * `UIImage.preparedForSending`: decode upright, cap the longest side at 2048px,
 * and re-encode as JPEG (which also drops EXIF/GPS metadata). GIFs pass through
 * untouched so animation survives. Decodes once for both the payload and the
 * thumbnail. Throws if the image can't be decoded.
 */
export async function prepareImageForSending(file: Blob): Promise<PreparedImage> {
  const bitmap = await createImageBitmap(file);
  try {
    const thumb = await encodeJpeg(bitmap, 320, 0.6);
    if (file.type === "image/gif") {
      return {
        bytes: new Uint8Array(await file.arrayBuffer()),
        contentType: file.type,
        width: bitmap.width,
        height: bitmap.height,
        thumbnail: Array.from(thumb.bytes),
      };
    }
    const full = await encodeJpeg(bitmap, OUTGOING_MAX_DIMENSION, OUTGOING_JPEG_QUALITY);
    return {
      bytes: full.bytes,
      contentType: "image/jpeg",
      width: full.width,
      height: full.height,
      thumbnail: Array.from(thumb.bytes),
    };
  } finally {
    bitmap.close();
  }
}

async function encodeJpeg(
  bitmap: ImageBitmap,
  maxDimension: number,
  quality: number
): Promise<{ bytes: Uint8Array; width: number; height: number }> {
  const scale = Math.min(1, maxDimension / Math.max(bitmap.width, bitmap.height));
  const width = Math.max(1, Math.round(bitmap.width * scale));
  const height = Math.max(1, Math.round(bitmap.height * scale));
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("no 2d canvas context");
  // JPEG has no alpha: paint transparent regions (e.g. PNG screenshots of
  // windows) white rather than letting them encode as black.
  ctx.fillStyle = "#fff";
  ctx.fillRect(0, 0, width, height);
  ctx.drawImage(bitmap, 0, 0, width, height);
  const blob = await new Promise<Blob>((resolve, reject) =>
    canvas.toBlob(
      (b) => (b ? resolve(b) : reject(new Error("canvas.toBlob returned null"))),
      "image/jpeg",
      quality
    )
  );
  return { bytes: new Uint8Array(await blob.arrayBuffer()), width, height };
}
