export interface ThumbnailResult {
  /** Downscaled JPEG bytes for the inline preview. */
  thumbnail: number[];
  /** Original image width/height (matches iOS `makeAttachmentThumbnail`). */
  width: number;
  height: number;
}

/**
 * Downscales an image file to a JPEG thumbnail via a `<canvas>`, mirroring iOS
 * `makeAttachmentThumbnail` (maxDimension 320, JPEG quality 0.6). Returns the
 * thumbnail bytes plus the *original* dimensions (the pointer carries the source
 * size, not the thumbnail size). Throws if the image can't be decoded.
 */
export async function makeImageThumbnail(
  file: Blob,
  maxDimension = 320,
  quality = 0.6
): Promise<ThumbnailResult> {
  const bitmap = await createImageBitmap(file);
  const width = bitmap.width;
  const height = bitmap.height;
  const scale = Math.min(1, maxDimension / Math.max(width, height));
  const tw = Math.max(1, Math.round(width * scale));
  const th = Math.max(1, Math.round(height * scale));

  const canvas = document.createElement("canvas");
  canvas.width = tw;
  canvas.height = th;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    bitmap.close();
    throw new Error("no 2d canvas context");
  }
  ctx.drawImage(bitmap, 0, 0, tw, th);
  bitmap.close();

  const blob = await new Promise<Blob>((resolve, reject) =>
    canvas.toBlob(
      (b) => (b ? resolve(b) : reject(new Error("canvas.toBlob returned null"))),
      "image/jpeg",
      quality
    )
  );
  const bytes = new Uint8Array(await blob.arrayBuffer());
  return { thumbnail: Array.from(bytes), width, height };
}
