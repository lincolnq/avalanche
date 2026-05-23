// logos.jsx — Avalanche logo glyph explorations
// Each glyph is a self-contained SVG component with viewBox 0 0 100 100,
// so it scales identically from favicon to billboard.

const P = {
  clay:     '#C9542B',   // primary warm clay
  deepClay: '#8C2E1A',   // darker terracotta
  amber:    '#D4A04A',   // warm amber accent
  moss:     '#6B7F4D',   // earthy moss accent
  ink:      '#1F1815',   // near-black, warm-biased
  cream:    '#F4EFE6',   // background cream
  paper:    '#E8E1D2',   // slightly tinted card surface
  hairline: 'rgba(31,24,21,0.10)',
};

// A bold sans capital A with an inner triangular hole.
// Outer outline (CW), then inner hole (CCW); use fillRule="evenodd".
const A_PATH = "M50 8 L88 90 L72 90 L65 73 L35 73 L28 90 L12 90 Z M50 32 L41 60 L59 60 Z";

function SVG({ children, size }) {
  return (
    <svg viewBox="0 0 100 100" width={size} height={size} style={{ display: 'block', overflow: 'visible' }}>
      {children}
    </svg>
  );
}

// ─── 1. Slatted A — letterform sliced by horizontal cuts ───
// Reads as: A made of avalanche-strata. Solid, anchored, momentum-implied.
function SlattedA({ size, color = P.clay }) {
  const id = React.useId();
  return (
    <SVG size={size}>
      <defs>
        <mask id={id}>
          <path d={A_PATH} fill="white" fillRule="evenodd" />
          <rect x="0" y="30"  width="100" height="2.4" fill="black" />
          <rect x="0" y="46"  width="100" height="2.4" fill="black" />
          <rect x="0" y="62"  width="100" height="2.4" fill="black" />
          <rect x="0" y="78"  width="100" height="2.4" fill="black" />
        </mask>
      </defs>
      <rect width="100" height="100" fill={color} mask={`url(#${id})`} />
    </SVG>
  );
}

// ─── 2. Dot A — letterform built from a swarm of voices ───
// Reads as: many people, one letter. Grassroots-coded.
function DotA({ size, color = P.clay }) {
  const dots = [];
  // diagonals: 7 dots each side, apex (50,12) to bases (16,86)/(84,86)
  for (let i = 0; i <= 6; i++) {
    const t = i / 6;
    dots.push([50 + (16 - 50) * t, 12 + (86 - 12) * t]);
    dots.push([50 + (84 - 50) * t, 12 + (86 - 12) * t]);
  }
  // crossbar: 4 dots
  for (let i = 0; i <= 3; i++) {
    const t = i / 3;
    dots.push([34 + (66 - 34) * t, 60]);
  }
  return (
    <SVG size={size}>
      {dots.map(([x, y], i) => (
        <circle key={i} cx={x} cy={y} r="5.6" fill={color} />
      ))}
    </SVG>
  );
}

// ─── 3. Faceted A — 2-tone folded paper A ───
// Reads as: solid, crystalline, slightly playful (origami).
function FacetA({ size, light = P.clay, dark = P.deepClay }) {
  const id = React.useId();
  return (
    <SVG size={size}>
      <defs>
        <mask id={id}>
          <path d={A_PATH} fill="white" fillRule="evenodd" />
        </mask>
      </defs>
      <g mask={`url(#${id})`}>
        <rect x="0"  y="0" width="50" height="100" fill={light} />
        <rect x="50" y="0" width="50" height="100" fill={dark} />
      </g>
    </SVG>
  );
}

// ─── 4. Echo A — letter in motion, trailed by ghosts ───
// Reads as: unstoppable, in flight, downhill.
function EchoA({ size, color = P.clay }) {
  return (
    <SVG size={size}>
      <g transform="translate(-12,-10)" opacity="0.18">
        <path d={A_PATH} fill={color} fillRule="evenodd" />
      </g>
      <g transform="translate(-6,-5)" opacity="0.4">
        <path d={A_PATH} fill={color} fillRule="evenodd" />
      </g>
      <path d={A_PATH} fill={color} fillRule="evenodd" />
    </SVG>
  );
}

// ─── 5. Bubble Cascade — chat bubbles tumbling down-right ───
// Reads as: messages cascading, chat-first, momentum.
function BubbleCascade({ size, color = P.clay }) {
  return (
    <SVG size={size}>
      {/* small (back) */}
      <g opacity="0.4">
        <rect x="8" y="12" width="26" height="18" rx="6" fill={color} />
        <path d="M14 28 L13 34 L20 30 Z" fill={color} />
      </g>
      {/* medium */}
      <g opacity="0.72">
        <rect x="26" y="34" width="40" height="26" rx="8" fill={color} />
        <path d="M34 58 L32 66 L42 60 Z" fill={color} />
      </g>
      {/* large (front) */}
      <g>
        <rect x="44" y="58" width="48" height="30" rx="9" fill={color} />
        <path d="M52 86 L48 94 L62 87 Z" fill={color} />
      </g>
    </SVG>
  );
}

// ─── 6. Pulse — concentric triangles, broadcast/signal ───
// Reads as: signal radiating, voice carrying.
function Pulse({ size, color = P.clay }) {
  return (
    <SVG size={size}>
      <polygon points="50,8 92,90 8,90"
        fill="none" stroke={color} strokeWidth="2.6" strokeLinejoin="round" opacity="0.22" />
      <polygon points="50,26 78,82 22,82"
        fill="none" stroke={color} strokeWidth="2.8" strokeLinejoin="round" opacity="0.5" />
      <polygon points="50,42 68,74 32,74" fill={color} />
    </SVG>
  );
}

// ─── 7. Wave Cone — signal/wave cascade in triangular silhouette ───
// Reads as: voice → many. Sound shaped like a slope.
function WaveCone({ size, color = P.clay }) {
  const bars = [];
  // Triangle apex (50,16), base y=86. At y, max halfWidth = (y-16)/70 * 38.
  const rows = [
    { y: 28, hw: 6  },
    { y: 40, hw: 12 },
    { y: 52, hw: 20 },
    { y: 64, hw: 28 },
    { y: 76, hw: 34 },
    { y: 88, hw: 40 },
  ];
  rows.forEach((r, i) => {
    bars.push(
      <rect key={i} x={50 - r.hw} y={r.y - 2.4} width={r.hw * 2} height="4.8" rx="2.4" fill={color} />
    );
  });
  return <SVG size={size}>{bars}</SVG>;
}

// ─── 8. Bubble Tri — speech-bubble + triangle hybrid ───
// Reads as: chat-app icon with momentum (downward tail).
function BubbleTri({ size, color = P.clay }) {
  // rounded-corner triangle with a speech tail off the bottom
  return (
    <SVG size={size}>
      <path d="
        M50 10
        Q56 10 58.5 15
        L88 76
        Q92 84 84 84
        L62 84
        L54 96
        L48 84
        L16 84
        Q8 84 12 76
        L41.5 15
        Q44 10 50 10 Z
      " fill={color} />
    </SVG>
  );
}

// ─── 9. Cluster — loose grassroots cluster of voices ───
// Reads as: many small things, organic, organizing.
function Cluster({ size, color = P.clay, accent = P.amber }) {
  const dots = [
    [50, 18, 9,  color],
    [30, 32, 7,  color],
    [70, 34, 6.5, accent],
    [18, 52, 7.5, color],
    [46, 50, 9,  accent],
    [73, 56, 8.5, color],
    [30, 72, 8,  color],
    [56, 74, 7,  accent],
    [80, 76, 6,  color],
    [12, 78, 5.5, color],
    [44, 88, 5,  color],
    [68, 90, 5,  accent],
  ];
  return (
    <SVG size={size}>
      {dots.map(([x, y, r, c], i) => (
        <circle key={i} cx={x} cy={y} r={r} fill={c} />
      ))}
    </SVG>
  );
}

// ─── 10. Tumble — three blocks cascading at angles ───
// Reads as: literal avalanche, abstract, playful.
function Tumble({ size, color = P.clay, accent = P.deepClay }) {
  return (
    <SVG size={size}>
      <g transform="translate(22,22) rotate(-18)">
        <rect x="-14" y="-14" width="28" height="28" rx="3" fill={color} opacity="0.45" />
      </g>
      <g transform="translate(48,48) rotate(12)">
        <rect x="-18" y="-18" width="36" height="36" rx="4" fill={color} opacity="0.78" />
      </g>
      <g transform="translate(74,78) rotate(-8)">
        <rect x="-15" y="-15" width="30" height="30" rx="3" fill={accent} />
      </g>
    </SVG>
  );
}

// ─── 11. Swarm — Sierpinski cluster of mini-triangles ───
// Reads as: collective force, fractal, secure (geometric).
function Swarm({ size, color = P.clay }) {
  const tri = (cx, cy, s, c, key) => {
    const h = s * 0.866;
    return (
      <polygon
        key={key}
        points={`${cx},${cy - h * 0.6} ${cx - s / 2},${cy + h * 0.4} ${cx + s / 2},${cy + h * 0.4}`}
        fill={c}
      />
    );
  };
  return (
    <SVG size={size}>
      {tri(50, 22, 26, color, 0)}
      {tri(33, 54, 26, color, 1)}
      {tri(67, 54, 26, color, 2)}
      {tri(16, 86, 26, color, 3)}
      {tri(50, 86, 26, color, 4)}
      {tri(84, 86, 26, color, 5)}
    </SVG>
  );
}

// ─── 12. Nodes — three-node network with link spine ───
// Reads as: encrypted graph, peer-to-peer, secure.
function Nodes({ size, color = P.clay, line = P.ink }) {
  return (
    <SVG size={size}>
      <g stroke={line} strokeWidth="2.8" strokeLinecap="round">
        <line x1="50" y1="22" x2="20" y2="74" />
        <line x1="50" y1="22" x2="80" y2="74" />
        <line x1="20" y1="74" x2="80" y2="74" />
      </g>
      <circle cx="50" cy="22" r="11" fill={color} />
      <circle cx="20" cy="74" r="11" fill={color} />
      <circle cx="80" cy="74" r="11" fill={color} />
    </SVG>
  );
}

// ─── Card: large mark on cream, scale strip on ink ───
function LogoCard({ name, code, desc, Glyph, glyphProps = {}, accent = P.clay }) {
  const G = Glyph;
  return (
    <div style={{
      width: 340, height: 420,
      display: 'flex', flexDirection: 'column',
      background: P.cream,
      fontFamily: '"Inter", "Helvetica Neue", system-ui, sans-serif',
      color: P.ink,
      borderRadius: 2,
    }}>
      {/* Glyph stage */}
      <div style={{
        flex: 1,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        position: 'relative',
        background: `linear-gradient(${P.cream}, ${P.paper})`,
      }}>
        {/* corner code */}
        <div style={{
          position: 'absolute', top: 14, left: 16,
          fontFamily: '"JetBrains Mono", ui-monospace, Menlo, monospace',
          fontSize: 10, letterSpacing: '0.12em', color: 'rgba(31,24,21,0.45)',
        }}>{code}</div>
        <div style={{
          position: 'absolute', top: 14, right: 16,
          fontFamily: '"JetBrains Mono", ui-monospace, Menlo, monospace',
          fontSize: 10, letterSpacing: '0.12em', color: 'rgba(31,24,21,0.35)',
        }}>AVL · 01</div>
        <G size={200} {...glyphProps} />
      </div>

      {/* Scale strip on dark */}
      <div style={{ background: P.ink, color: P.cream, padding: '16px 20px 18px' }}>
        <div style={{
          display: 'flex', alignItems: 'baseline', justifyContent: 'space-between',
          marginBottom: 14,
        }}>
          <div style={{
            fontSize: 13, fontWeight: 600, letterSpacing: '0.02em',
          }}>{name}</div>
          <div style={{
            fontSize: 9, opacity: 0.45, letterSpacing: '0.18em',
            fontFamily: '"JetBrains Mono", ui-monospace, Menlo, monospace',
          }}>SCALE TEST</div>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', width: 56, height: 56 }}>
            <G size={48} {...glyphProps} />
          </div>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', width: 28, height: 56 }}>
            <G size={24} {...glyphProps} />
          </div>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', width: 16, height: 56 }}>
            <G size={14} {...glyphProps} />
          </div>
          <div style={{
            flex: 1, marginLeft: 4,
            fontSize: 11, lineHeight: 1.45, opacity: 0.65,
            textWrap: 'pretty',
          }}>{desc}</div>
        </div>
      </div>
    </div>
  );
}

Object.assign(window, {
  AvlPalette: P,
  LogoCard,
  SlattedA, DotA, FacetA, EchoA,
  BubbleCascade, Pulse, WaveCone, BubbleTri,
  Cluster, Tumble, Swarm, Nodes,
});
