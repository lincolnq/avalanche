// logos-lowera.jsx — lowercase 'a' + chat-bubble tail fusions.
// Two glyph traditions for lowercase a: double-story (text-like) and
// single-story (geometric, e.g. Futura). Try both, with tails.

const P_LA = window.AvlPalette;

function SVG_LA({ children, size }) {
  return (
    <svg viewBox="0 0 100 100" width={size} height={size} style={{ display: 'block', overflow: 'visible' }}>
      {children}
    </svg>
  );
}

// ─── 1. SINGLE-STORY · Tail under the bowl ────────────────────────
// A circular bowl with a thick right-side stem; chat tail drops
// straight off the bottom of the bowl. Most "bubble-like" of the set
// because the bowl already IS a circle.
function LowerA_Single({ size, color = P_LA.clay }) {
  return (
    <SVG_LA size={size}>
      {/* bowl */}
      <circle cx="45" cy="55" r="33" fill={color}/>
      {/* stem */}
      <rect x="70" y="22" width="14" height="66" rx="2" fill={color}/>
      {/* knock out the hole that makes it an 'a' (counter) */}
      <circle cx="45" cy="55" r="14" fill={P_LA.cream}/>
      {/* chat tail off lower-left */}
      <path d="M22 78 L8 96 L36 84 Z" fill={color}/>
    </SVG_LA>
  );
}

// ─── 2. SINGLE-STORY · Bubble-a ──────────────────────────────────
// Same geometry, but the tail attaches to the corner where stem
// meets bowl — looks more obviously like a speech bubble.
function LowerA_Bubble({ size, color = P_LA.clay }) {
  return (
    <SVG_LA size={size}>
      <circle cx="44" cy="52" r="34" fill={color}/>
      <rect x="70" y="18" width="14" height="68" rx="2" fill={color}/>
      <circle cx="44" cy="52" r="15" fill={P_LA.cream}/>
      {/* tail off bottom-right of bowl, just before stem */}
      <path d="M60 80 L66 96 L74 82 Z" fill={color}/>
    </SVG_LA>
  );
}

// ─── 3. DOUBLE-STORY · Text-a with tail ──────────────────────────
// Familiar typographic 'a' shape (top arc + bowl + stem-spur).
// Tail off the bottom-left makes it read as a chat bubble.
// Built with strokes for clarity at scale.
function LowerA_Double({ size, color = P_LA.clay }) {
  return (
    <SVG_LA size={size}>
      {/* bowl */}
      <path
        d="M30 56
           Q30 36 50 36
           Q70 36 70 56
           L70 76
           Q70 84 60 84
           Q50 84 50 76
           Q50 68 60 68
           Q70 68 70 60"
        fill="none" stroke={color} strokeWidth="11" strokeLinecap="round" strokeLinejoin="round"
      />
      {/* right stem-spur (the vertical down the right side) */}
      <path
        d="M72 38 L72 84"
        fill="none" stroke={color} strokeWidth="11" strokeLinecap="round"
      />
      {/* chat tail off lower-left */}
      <path d="M28 78 L14 96 L42 84 Z" fill={color}/>
    </SVG_LA>
  );
}

// ─── 4. A-AS-BUBBLE ──────────────────────────────────────────────
// Treat the lowercase a's overall silhouette as the chat bubble itself.
// Rounded rect with a counter punched out and a tail. The counter's
// position makes it read as 'a' without spelling it letter-by-letter.
function LowerA_AsBubble({ size, color = P_LA.clay }) {
  return (
    <SVG_LA size={size}>
      <path d="
        M28 18
        L72 18
        Q86 18 86 32
        L86 76
        Q86 84 78 84
        L60 84
        L54 96
        L48 84
        L22 84
        Q14 84 14 76
        L14 32
        Q14 18 28 18 Z
        M38 46
        Q38 38 50 38
        Q62 38 62 46
        L62 66
        Q62 72 56 72
        Q50 72 50 66
        Q50 60 56 60
        Q62 60 62 56
      " fill={color} fillRule="evenodd"/>
    </SVG_LA>
  );
}

Object.assign(window, {
  LowerA_Single, LowerA_Bubble, LowerA_Double, LowerA_AsBubble,
});
