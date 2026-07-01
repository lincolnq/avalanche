import { createMemo, Show } from "solid-js";
import qrcode from "qrcode-generator";
import "./QRCode.css";

// Themed QR rendering for the device-linking flow (and any future invite QR).
// Mirrors iOS `QRCodeView` and Android `QRCodeImage` in appearance: foreground
// Plum800 on a fixed Paper backing so the code stays scannable in dark mode — a
// dark-on-dark QR can't be read. Uses `qrcode-generator` (zero-dependency,
// synchronous); we render the module grid to an SVG ourselves for exact
// theming and to keep the payload out of the markup.
const FG = "#3C1F2D"; // plum800
const BG = "#FFF1E9"; // sand100 / avPaper
const MARGIN = 2; // quiet zone, in modules

export default function QRCode(props: { text: string; size?: number }) {
  const svg = createMemo(() => {
    if (!props.text) return "";
    try {
      const qr = qrcode(0, "M"); // type 0 = auto-size for the payload; ECC level M
      qr.addData(props.text);
      qr.make();
      const count = qr.getModuleCount();
      const dim = count + MARGIN * 2;
      let modules = "";
      for (let row = 0; row < count; row++) {
        for (let col = 0; col < count; col++) {
          if (qr.isDark(row, col)) {
            modules += `<rect x="${col + MARGIN}" y="${row + MARGIN}" width="1" height="1"/>`;
          }
        }
      }
      return (
        `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${dim} ${dim}" ` +
        `shape-rendering="crispEdges" preserveAspectRatio="xMidYMid meet">` +
        `<rect width="${dim}" height="${dim}" fill="${BG}"/>` +
        `<g fill="${FG}">${modules}</g></svg>`
      );
    } catch {
      // Payload too large for a single symbol, or an encoder error — render
      // nothing; callers keep the copyable text code as the fallback.
      return "";
    }
  });

  const px = () => `${props.size ?? 220}px`;

  return (
    <Show when={svg()}>
      {/* The SVG is built from the QR module grid (numeric coordinates + fixed
          colors); `props.text` never enters the markup, so there's no injection
          surface here. */}
      <div class="qr-code" style={{ width: px(), height: px() }} innerHTML={svg()} />
    </Show>
  );
}
