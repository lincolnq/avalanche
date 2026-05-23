// logos-abubble.jsx — Bubble-Tri × A fusion attempts.
// The brief: a chat bubble that also reads as the letter A.
// Four ways in, ordered subtle → bold.

const P_AB = window.AvlPalette;

function SVG_AB({ children, size }) {
  return (
    <svg viewBox="0 0 100 100" width={size} height={size} style={{ display: 'block', overflow: 'visible' }}>
      {children}
    </svg>
  );
}

// ─── 1. COUNTER ────────────────────────────────────────────────
// The original Bubble-Tri silhouette with A's inner counter cut out.
// The bottom edge of the counter does the work of the A's crossbar.
// Most subtle of the fusions; reads as bubble first, A second.
function ABubble_Counter({ size, color = P_AB.clay }) {
  return (
    <SVG_AB size={size}>
      <path d="
        M50 12 Q56 12 58.5 17
        L88 76 Q92 84 84 84
        L60 84 L54 96 L48 84
        L16 84 Q8 84 12 76
        L41.5 17 Q44 12 50 12 Z
        M50 36 L42 64 L58 64 Z
      " fill={color} fillRule="evenodd"/>
    </SVG_AB>
  );
}

// ─── 2. OPEN ───────────────────────────────────────────────────
// A full A letterform with bubble-rounded corners and a chat tail
// off the right foot. The legs stay separated (open bottom) so the
// A reads unmistakably. Tail moves the silhouette into "bubble".
function ABubble_Open({ size, color = P_AB.clay }) {
  return (
    <SVG_AB size={size}>
      <path d="
        M50 10
        Q57 10 60 17
        L87 84
        Q91 90 86 90
        L80 90
        L75 100
        L70 90
        L68 90
        Q63 90 66 84
        L64 76
        Q64 73 60 73
        L40 73
        Q36 73 36 76
        L34 84
        Q37 90 32 90
        L14 90
        Q9 90 13 84
        L40 17
        Q43 10 50 10 Z
        M50 32 L41 60 L59 60 Z
      " fill={color} fillRule="evenodd"/>
    </SVG_AB>
  );
}

// ─── 3. HOLLOW ─────────────────────────────────────────────────
// Outline form of Counter, with a small filled bubble at the
// center of the implied crossbar. Reads as "voice inside an A".
function ABubble_Hollow({ size, color = P_AB.clay }) {
  return (
    <SVG_AB size={size}>
      <path d="
        M50 12 Q56 12 58.5 17
        L88 76 Q92 84 84 84
        L60 84 L54 96 L48 84
        L16 84 Q8 84 12 76
        L41.5 17 Q44 12 50 12 Z
        M50 36 L42 64 L58 64 Z
      " fill="none" stroke={color} strokeWidth="5" strokeLinejoin="round" fillRule="evenodd"/>
      {/* the "voice" sitting on the implied crossbar */}
      <circle cx="50" cy="73" r="6.5" fill={color}/>
    </SVG_AB>
  );
}

// ─── 4. STENCIL ────────────────────────────────────────────────
// Solid bubble silhouette with BOTH the A counter AND a clear
// horizontal crossbar slot cut out. Boldest, most-A read.
function ABubble_Stencil({ size, color = P_AB.clay }) {
  return (
    <SVG_AB size={size}>
      <path d="
        M50 12 Q56 12 58.5 17
        L88 76 Q92 84 84 84
        L60 84 L54 96 L48 84
        L16 84 Q8 84 12 76
        L41.5 17 Q44 12 50 12 Z
        M50 30 L42 56 L58 56 Z
        M36 62 L64 62 L62 70 L38 70 Z
      " fill={color} fillRule="evenodd"/>
    </SVG_AB>
  );
}

Object.assign(window, {
  ABubble_Counter, ABubble_Open, ABubble_Hollow, ABubble_Stencil,
});
