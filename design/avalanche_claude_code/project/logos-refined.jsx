// logos-refined.jsx — Round 2: variations of Slatted A, Wave Cone, Bubble-Tri,
// plus non-bubble takes on the cascade idea.

const P_R = window.AvlPalette;
const A_PATH_R = "M50 8 L88 90 L72 90 L65 73 L35 73 L28 90 L12 90 Z M50 32 L41 60 L59 60 Z";

function SVG_R({ children, size }) {
  return (
    <svg viewBox="0 0 100 100" width={size} height={size} style={{ display: 'block', overflow: 'visible' }}>
      {children}
    </svg>
  );
}

// ═════════════════════════════════════════════════════════════════════
// SLATTED A — refinements
// ═════════════════════════════════════════════════════════════════════

// 6 thin slats — dense, technical, hi-res feel
function SlattedA_Tight({ size, color = P_R.clay }) {
  const id = React.useId();
  return (
    <SVG_R size={size}>
      <defs>
        <mask id={id}>
          <path d={A_PATH_R} fill="white" fillRule="evenodd" />
          {[22, 33, 44, 55, 66, 77, 88].map((y, i) => (
            <rect key={i} x="0" y={y} width="100" height="1.8" fill="black" />
          ))}
        </mask>
      </defs>
      <rect width="100" height="100" fill={color} mask={`url(#${id})`} />
    </SVG_R>
  );
}

// 3 wide slats — bolder, more graphic
function SlattedA_Wide({ size, color = P_R.clay }) {
  const id = React.useId();
  return (
    <SVG_R size={size}>
      <defs>
        <mask id={id}>
          <path d={A_PATH_R} fill="white" fillRule="evenodd" />
          {[38, 60, 82].map((y, i) => (
            <rect key={i} x="0" y={y} width="100" height="3.6" fill="black" />
          ))}
        </mask>
      </defs>
      <rect width="100" height="100" fill={color} mask={`url(#${id})`} />
    </SVG_R>
  );
}

// Slats widen toward bottom — implies an actual cascade / accumulation
function SlattedA_Taper({ size, color = P_R.clay }) {
  const id = React.useId();
  return (
    <SVG_R size={size}>
      <defs>
        <mask id={id}>
          <path d={A_PATH_R} fill="white" fillRule="evenodd" />
          {[
            { y: 26, h: 1.4 },
            { y: 42, h: 1.9 },
            { y: 58, h: 2.6 },
            { y: 74, h: 3.4 },
            { y: 90, h: 4.2 },
          ].map((s, i) => (
            <rect key={i} x="0" y={s.y} width="100" height={s.h} fill="black" />
          ))}
        </mask>
      </defs>
      <rect width="100" height="100" fill={color} mask={`url(#${id})`} />
    </SVG_R>
  );
}

// Slats slightly tilted — downhill motion
function SlattedA_Angled({ size, color = P_R.clay }) {
  const id = React.useId();
  return (
    <SVG_R size={size}>
      <defs>
        <mask id={id}>
          <path d={A_PATH_R} fill="white" fillRule="evenodd" />
          <g transform="rotate(-7 50 50)">
            {[30, 46, 62, 78].map((y, i) => (
              <rect key={i} x="-20" y={y} width="140" height="2.4" fill="black" />
            ))}
          </g>
        </mask>
      </defs>
      <rect width="100" height="100" fill={color} mask={`url(#${id})`} />
    </SVG_R>
  );
}

// ═════════════════════════════════════════════════════════════════════
// WAVE CONE — refinements
// ═════════════════════════════════════════════════════════════════════

// Tighter bars — denser signal cone
function WaveCone_Tight({ size, color = P_R.clay }) {
  const rows = [
    { y: 22, hw: 4 },
    { y: 31, hw: 9 },
    { y: 40, hw: 15 },
    { y: 49, hw: 21 },
    { y: 58, hw: 27 },
    { y: 67, hw: 32 },
    { y: 76, hw: 36 },
    { y: 85, hw: 40 },
  ];
  return (
    <SVG_R size={size}>
      {rows.map((r, i) => (
        <rect key={i} x={50 - r.hw} y={r.y - 1.8} width={r.hw * 2} height="3.6" rx="1.8" fill={color}/>
      ))}
    </SVG_R>
  );
}

// Solid apex triangle + fanning bars — clear broadcast source
function WaveCone_Apex({ size, color = P_R.clay }) {
  return (
    <SVG_R size={size}>
      <polygon points="50,14 58,30 42,30" fill={color}/>
      {[
        { y: 38, hw: 12 },
        { y: 50, hw: 20 },
        { y: 62, hw: 28 },
        { y: 74, hw: 34 },
        { y: 86, hw: 40 },
      ].map((r, i) => (
        <rect key={i} x={50 - r.hw} y={r.y - 2.4} width={r.hw * 2} height="4.8" rx="2.4" fill={color}/>
      ))}
    </SVG_R>
  );
}

// Voice-waveform — widths not strictly monotonic; reads as audio
function WaveCone_Voice({ size, color = P_R.clay }) {
  const bars = [
    { y: 22, hw: 6  },
    { y: 34, hw: 18 },
    { y: 46, hw: 13 },
    { y: 58, hw: 26 },
    { y: 70, hw: 22 },
    { y: 82, hw: 38 },
  ];
  return (
    <SVG_R size={size}>
      {bars.map((b, i) => (
        <rect key={i} x={50 - b.hw} y={b.y - 2.6} width={b.hw * 2} height="5.2" rx="2.6" fill={color}/>
      ))}
    </SVG_R>
  );
}

// Diamond — two cones touching, signal both directions
function WaveCone_Diamond({ size, color = P_R.clay }) {
  // upper cone: apex at top, widening to middle
  // lower cone: widening from middle, narrowing to bottom
  const top = [
    { y: 16, hw: 4  },
    { y: 26, hw: 12 },
    { y: 36, hw: 22 },
    { y: 46, hw: 32 },
  ];
  const bot = [
    { y: 56, hw: 32 },
    { y: 66, hw: 22 },
    { y: 76, hw: 12 },
    { y: 86, hw: 4  },
  ];
  return (
    <SVG_R size={size}>
      {[...top, ...bot].map((r, i) => (
        <rect key={i} x={50 - r.hw} y={r.y - 2.2} width={r.hw * 2} height="4.4" rx="2.2" fill={color}/>
      ))}
    </SVG_R>
  );
}

// ═════════════════════════════════════════════════════════════════════
// BUBBLE-TRI — refinements
// ═════════════════════════════════════════════════════════════════════

// Sharper corners — more triangle, less bubble
function BubbleTri_Sharp({ size, color = P_R.clay }) {
  return (
    <SVG_R size={size}>
      <path d="
        M50 8
        Q53 8 54.5 11
        L90 78
        Q92.5 84 87 84
        L62 84
        L54 96
        L48 84
        L13 84
        Q7.5 84 10 78
        L45.5 11
        Q47 8 50 8 Z
      " fill={color} />
    </SVG_R>
  );
}

// Outline + inner mini-bubble — secure/encrypted speech read
function BubbleTri_Hollow({ size, color = P_R.clay }) {
  return (
    <SVG_R size={size}>
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
      " fill="none" stroke={color} strokeWidth="5" strokeLinejoin="round"/>
      {/* inner mini bubble */}
      <circle cx="50" cy="56" r="9" fill={color}/>
    </SVG_R>
  );
}

// Two overlapping bubble-tris — dialogue / community
function BubbleTri_Dialogue({ size, color = P_R.clay, accent = P_R.deepClay }) {
  const path = `
    M0 -42
    Q6 -42 8.5 -37
    L38 24
    Q42 32 34 32
    L12 32
    L4 44
    L-2 32
    L-34 32
    Q-42 32 -38 24
    L-8.5 -37
    Q-6 -42 0 -42 Z
  `;
  return (
    <SVG_R size={size}>
      <g transform="translate(34, 46) scale(0.62)" opacity="0.85">
        <path d={path} fill={accent}/>
      </g>
      <g transform="translate(64, 56) scale(0.62)">
        <path d={path} fill={color}/>
      </g>
    </SVG_R>
  );
}

// Tail on side, asymmetric — slightly off-axis, more dynamic
function BubbleTri_Tilt({ size, color = P_R.clay }) {
  return (
    <SVG_R size={size}>
      <g transform="rotate(-6 50 52)">
        <path d="
          M50 10
          Q56 10 58.5 15
          L88 76
          Q92 84 84 84
          L26 84
          Q18 84 22 76
          L41.5 15
          Q44 10 50 10 Z
        " fill={color} />
        {/* off-axis tail */}
        <path d="M28 80 L14 94 L36 82 Z" fill={color}/>
      </g>
    </SVG_R>
  );
}

// ═════════════════════════════════════════════════════════════════════
// CASCADE — non-bubble takes on the cascading idea
// ═════════════════════════════════════════════════════════════════════

// Three nested triangles cascading down-right
function CascadeTri({ size, color = P_R.clay }) {
  const tri = (cx, cy, s, op) => {
    const h = s * 0.866;
    return (
      <polygon
        points={`${cx},${cy - h * 0.55} ${cx - s / 2},${cy + h * 0.45} ${cx + s / 2},${cy + h * 0.45}`}
        fill={color} opacity={op}
      />
    );
  };
  return (
    <SVG_R size={size}>
      {tri(22, 24, 22, 0.34)}
      {tri(46, 48, 32, 0.65)}
      {tri(72, 76, 42, 1)}
    </SVG_R>
  );
}

// Three mini A's cascading — letterform + momentum
function CascadeAA({ size, color = P_R.clay }) {
  const a = (tx, ty, scale, op) => (
    <g transform={`translate(${tx},${ty}) scale(${scale}) translate(-12,-8)`} opacity={op}>
      <path d={A_PATH_R} fill={color} fillRule="evenodd" />
    </g>
  );
  // A_PATH bounds x∈[12,88] (w=76), y∈[8,90] (h=82). After translate(-12,-8) origin is top-left of A.
  return (
    <SVG_R size={size}>
      {a(2,  2,  0.30, 0.34)}
      {a(26, 26, 0.42, 0.66)}
      {a(50, 46, 0.55, 1)}
    </SVG_R>
  );
}

// Horizontal strata stepping down + right — like avalanche layers settling
function CascadeStrata({ size, color = P_R.clay }) {
  const bars = [
    { x: 18, y: 14, w: 22, op: 0.30 },
    { x: 26, y: 26, w: 32, op: 0.45 },
    { x: 18, y: 38, w: 48, op: 0.60 },
    { x: 24, y: 52, w: 54, op: 0.75 },
    { x: 16, y: 66, w: 66, op: 0.88 },
    { x: 12, y: 82, w: 76, op: 1    },
  ];
  return (
    <SVG_R size={size}>
      {bars.map((b, i) => (
        <rect key={i} x={b.x} y={b.y - 3} width={b.w} height="6" rx="3" fill={color} opacity={b.op}/>
      ))}
    </SVG_R>
  );
}

// Particle storm — dense at the bottom, sparse at the top
function CascadeMass({ size, color = P_R.clay }) {
  // Deterministic LCG seeded so the output is stable across renders.
  let seed = 0x1f1a83;
  const rnd = () => {
    seed = (seed * 1103515245 + 12345) & 0x7fffffff;
    return ((seed >> 8) % 100000) / 100000;
  };
  const dots = [];
  const cells = [
    { y0: 8,  y1: 28, count: 4  },
    { y0: 28, y1: 50, count: 10 },
    { y0: 50, y1: 72, count: 16 },
    { y0: 72, y1: 94, count: 22 },
  ];
  cells.forEach(({ y0, y1, count }) => {
    for (let i = 0; i < count; i++) {
      const x = 6 + rnd() * 88;
      const y = y0 + rnd() * (y1 - y0);
      const r = 1.4 + rnd() * 2.2;
      dots.push([x, y, r]);
    }
  });
  return (
    <SVG_R size={size}>
      {dots.map(([x, y, r], i) => <circle key={i} cx={x} cy={y} r={r} fill={color}/>)}
    </SVG_R>
  );
}

Object.assign(window, {
  SlattedA_Tight, SlattedA_Wide, SlattedA_Taper, SlattedA_Angled,
  WaveCone_Tight, WaveCone_Apex, WaveCone_Voice, WaveCone_Diamond,
  BubbleTri_Sharp, BubbleTri_Hollow, BubbleTri_Dialogue, BubbleTri_Tilt,
  CascadeTri, CascadeAA, CascadeStrata, CascadeMass,
});
