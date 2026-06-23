import { createSignal } from "solid-js";
import jsQR from "jsqr";
import { useApp } from "../../state/AppContext";
import type { InviteInfo } from "../../models/InviteToken";
import { useInviteValidation } from "../../lib/useInviteValidation";
import "./QRScannerView.css";

async function decodeQRFile(file: File): Promise<string | null> {
  return new Promise((resolve) => {
    const img = new Image();
    const url = URL.createObjectURL(file);
    img.onload = () => {
      const canvas = document.createElement("canvas");
      canvas.width = img.width;
      canvas.height = img.height;
      const ctx = canvas.getContext("2d");
      if (!ctx) { URL.revokeObjectURL(url); resolve(null); return; }
      ctx.drawImage(img, 0, 0);
      const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);
      const code = jsQR(imageData.data, imageData.width, imageData.height);
      URL.revokeObjectURL(url);
      resolve(code?.data ?? null);
    };
    img.onerror = () => { URL.revokeObjectURL(url); resolve(null); };
    img.src = url;
  });
}

interface Props {
  onValidated: (info: InviteInfo) => void;
  onBack?: () => void;
}

export default function QRScannerView(props: Props) {
  const { validateInvite } = useApp();
  const [linkText, setLinkText] = createSignal("");
  const [isDragOver, setIsDragOver] = createSignal(false);
  let fileInputRef: HTMLInputElement | undefined;

  const { error, setError, isValidating, setIsValidating, validate: validateToken } = useInviteValidation(
    validateInvite,
    props.onValidated,
    "Invalid invite"
  );

  async function handleFile(file: File) {
    setError(null);
    setIsValidating(true);
    let decoded: string | null;
    try {
      decoded = await decodeQRFile(file);
    } catch {
      setError("Couldn't read that image");
      setIsValidating(false);
      return;
    }
    if (!decoded) {
      setError("No QR code found in image");
      setIsValidating(false);
      return;
    }
    // Hand off to the hook: it resets isValidating in its own finally block.
    setIsValidating(false);
    await validateToken(decoded);
  }

  function handleFileInput(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (file) void handleFile(file);
    input.value = "";
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault();
    setIsDragOver(false);
    const file = e.dataTransfer?.files[0];
    if (file) void handleFile(file);
  }

  return (
    <div class="qr-scanner">
        <div class="qs-title">Scan Invite QR</div>
        <div class="qs-subtitle">Upload a QR code image, or paste your invite link below.</div>

        <input
          ref={fileInputRef}
          class="qs-file-input"
          type="file"
          accept="image/*"
          onChange={handleFileInput}
        />
        <div
          class={`qs-drop-zone${isDragOver() ? " drag-over" : ""}`}
          onClick={() => fileInputRef?.click()}
          onDragOver={(e) => { e.preventDefault(); setIsDragOver(true); }}
          onDragLeave={() => setIsDragOver(false)}
          onDrop={handleDrop}
        >
          <div class="qs-drop-icon">📷</div>
          <div class="qs-drop-label">Click or drop QR image here</div>
          <div class="qs-drop-hint">PNG, JPG, or any image format</div>
        </div>

        <div class="qs-divider">or paste link</div>

        <div class="qs-paste-label">Invite link or token</div>
        <input
          class="text-input qs-input"
          type="text"
          placeholder="actnet://... or bare token"
          value={linkText()}
          onInput={(e) => setLinkText(e.currentTarget.value)}
          onKeyDown={(e) => { if (e.key === "Enter") void validateToken(linkText()); }}
        />

        {error() && <div class="qs-error">{error()}</div>}

        <button
          class="btn-primary qs-btn"
          disabled={!linkText().trim() || isValidating()}
          onClick={() => void validateToken(linkText())}
        >
          {isValidating() ? "Validating…" : "Continue"}
        </button>

        {props.onBack && (
          <button class="back-btn qs-back" onClick={props.onBack}>← Back</button>
        )}
    </div>
  );
}
