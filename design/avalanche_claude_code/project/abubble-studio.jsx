// abubble-studio.jsx — focused studio for the user's custom a-bubble.
// Their original path is preserved verbatim and used as the base.

const P_S = window.AvlPalette;

// Original path from the user's wordmark SVG (id="path-1"), verbatim.
const A_BASE =
  "M31.9518027,136.214416 C35.4306983,136.214416 39.4286804,135.058161 45,135.058161 " +
  "C46.4948695,135.058161 48.2201557,135.340482 50.3272995,138 C52.6362313,140.914203 55.0224275,143.487955 56.8316034,143.487955 " +
  "C60.2918431,143.487955 58.5421449,135.058161 62.6620054,135.058161 L82.7869249,135.058161 " +
  "C85.1041162,135.058161 87,133.166979 87,130.855535 L87,63.8236398 " +
  "C87,41.1294559 72.2542373,26 45.5012107,26 C19.1694915,26 4.75042526,43.1637576 3.21307506,58.1500938 " +
  "C2.51,65.0037822 0,83.9962477 0,102.697936 C0,123.711069 15.1529076,136.214416 31.9518027,136.214416 Z";

// Variants of the base where only the bottom tail segment is altered.
// Original tail: (45,135.06) → (50.33,138) → (56.83,143.49) → (58.54,135.06) → (62.66,135.06)

// Longer, more pronounced tail
const A_TAIL_LONG =
  "M31.9518027,136.214416 C35.4306983,136.214416 39.4286804,135.058161 45,135.058161 " +
  "C46.5,135.058161 48,135.6 50,140 C53,146 56,151 58,151 " +
  "C61.5,151 60,135.058161 64,135.058161 L82.7869249,135.058161 " +
  "C85.1041162,135.058161 87,133.166979 87,130.855535 L87,63.8236398 " +
  "C87,41.1294559 72.2542373,26 45.5012107,26 C19.1694915,26 4.75042526,43.1637576 3.21307506,58.1500938 " +
  "C2.51,65.0037822 0,83.9962477 0,102.697936 C0,123.711069 15.1529076,136.214416 31.9518027,136.214416 Z";

// Sharp, geometric tail (no curvature)
const A_TAIL_SHARP =
  "M31.9518027,136.214416 C35.4306983,136.214416 39.4286804,135.058161 45,135.058161 " +
  "L46,135.058161 L57,146 L60,135.058161 L82.7869249,135.058161 " +
  "C85.1041162,135.058161 87,133.166979 87,130.855535 L87,63.8236398 " +
  "C87,41.1294559 72.2542373,26 45.5012107,26 C19.1694915,26 4.75042526,43.1637576 3.21307506,58.1500938 " +
  "C2.51,65.0037822 0,83.9962477 0,102.697936 C0,123.711069 15.1529076,136.214416 31.9518027,136.214416 Z";

// Tail moved to the LEFT side (and the bubble mirrored about x=43.5 implicitly via path)
// For simplicity I'll shift the tail features to the left of center.
const A_TAIL_LEFT =
  "M31.9518027,136.214416 C35.4306983,136.214416 38,135.06 38,135.058161 " +
  "C41.5,135.058161 40,143.49 42.5,143.49 C44.3,143.49 46.7,140.91 49,138 " +
  "C51.1,135.34 52.8,135.058161 54,135.058161 C56,135.058161 60,136.21 65,136.21 " +
  "L82.7869249,135.058161 C85.1041162,135.058161 87,133.166979 87,130.855535 L87,63.8236398 " +
  "C87,41.1294559 72.2542373,26 45.5012107,26 C19.1694915,26 4.75042526,43.1637576 3.21307506,58.1500938 " +
  "C2.51,65.0037822 0,83.9962477 0,102.697936 C0,123.711069 15.1529076,136.214416 31.9518027,136.214416 Z";

// ─── Base glyph component (aspect-preserving) ─────────────────
// The path natively lives roughly in x[0,87], y[26,143]. We render
// with a fixed *height*, letting width follow the 87:117.49 aspect.
function ABG({ height = 200, color, path = A_BASE, counter, voice, outline, strokeWidth = 7 }) {
  const aspectW = 87 / 117.49;
  const w = height * aspectW;
  // viewBox padded a hair so any strokes don't clip
  const vb = "-5 21 97 127";
  // If we want an 'a' counter (the inner hole), we add an inner subpath
  // with opposite winding. We use evenodd to keep it bg-agnostic.
  let d = path;
  if (counter) {
    // a small bean-shaped hole roughly where an 'a' counter would sit
    d += " M55,85 C55,77 47,73 39,73 C29,73 22,80 22,92 C22,103 30,110 39,110 C46,110 55,106 55,98 Z";
  }
  return (
    <svg viewBox={vb} width={w} height={height} style={{ display: 'block' }}>
      {outline ? (
        <path d={path} fill="none" stroke={color} strokeWidth={strokeWidth} strokeLinejoin="round"/>
      ) : (
        <path d={d} fill={color} fillRule="evenodd"/>
      )}
      {voice && (
        <g fill={voice}>
          <circle cx="30" cy="88" r="5"/>
          <circle cx="45" cy="88" r="5"/>
          <circle cx="60" cy="88" r="5"/>
        </g>
      )}
    </svg>
  );
}

window.ABubbleStudio = { A_BASE, A_TAIL_LONG, A_TAIL_SHARP, A_TAIL_LEFT, ABG };
