/** SVG shield icon that fills proportionally to trust level 0-4. */

const TRUST_META: Record<number, { name: string; color: string }> = {
  0: { name: 'Unknown',      color: '#8090a0' },  // gray
  1: { name: 'Acquaintance', color: '#53c0f0' },  // blue
  2: { name: 'Friend',       color: '#53c0f0' },  // blue
  3: { name: 'Trusted',      color: '#50c878' },  // green
  4: { name: 'Inner Circle', color: '#f0c050' },  // gold
};

export function trustMeta(level: number) {
  return TRUST_META[level] ?? TRUST_META[0];
}

export function TrustShield({
  level,
  size = 18,
  onClick,
}: {
  level: number;
  size?: number;
  onClick?: (e: React.MouseEvent) => void;
}) {
  const { color } = trustMeta(level);
  // Fill fraction: 0→0%, 1→25%, 2→50%, 3→75%, 4→100%
  const fillFraction = level / 4;
  // Shield path viewBox is 0 0 20 24
  const shieldH = 20; // total shield body height in viewBox units
  const fillY = 2 + shieldH * (1 - fillFraction);
  const fillH = shieldH * fillFraction;

  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 20 24"
      style={{ cursor: onClick ? 'pointer' : 'default', flexShrink: 0 }}
      onClick={onClick}
      aria-label={`Trust: ${trustMeta(level).name}`}
    >
      {/* Clip path for the shield shape */}
      <defs>
        <clipPath id={`shield-clip-${level}-${size}`}>
          <path d="M10 1 L18 5 L18 13 C18 18 10 23 10 23 C10 23 2 18 2 13 L2 5 Z" />
        </clipPath>
      </defs>

      {/* Fill rectangle (clipped to shield) */}
      {fillFraction > 0 && (
        <rect
          x="0"
          y={fillY}
          width="20"
          height={fillH + 2}
          fill={color}
          opacity={0.35}
          clipPath={`url(#shield-clip-${level}-${size})`}
        />
      )}

      {/* Shield outline */}
      <path
        d="M10 1 L18 5 L18 13 C18 18 10 23 10 23 C10 23 2 18 2 13 L2 5 Z"
        fill="none"
        stroke={color}
        strokeWidth="1.5"
      />
    </svg>
  );
}
