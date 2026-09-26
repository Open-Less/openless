import { useId, type CSSProperties } from 'react';

export type BuddyColor = 'coral' | 'amber' | 'violet' | 'mint';
const COLORS: Record<BuddyColor, [string, string]> = {
  coral: ['#f5b59b', '#dd8167'],
  amber: ['#f8d99a', '#d6af65'],
  violet: ['#c9b7ea', '#a18bc8'],
  mint: ['#b9d5af', '#81b095'],
};
const SHAPES: Record<BuddyColor, string> = {
  coral: 'M19 11c6-6 21-6 26 1l7 9c9 11 4 28-9 31l-6 1-8 6-2-6C9 51 4 40 8 28l3-9z',
  amber:
    'M16 9c-3-5-8-2-7 3l2 13C2 36 8 51 23 53l10 5 3-6c15 0 23-12 19-25l-5-13c-1-9-9-11-10-2-8-4-17-5-24-3z',
  violet:
    'M32 6 52 16c6 3 7 9 5 15l-4 14c-2 8-11 11-19 10l-9 4-2-7C9 49 4 40 7 28l4-11C14 10 23 5 32 6z',
  mint: 'M31 11c1-9 8-9 10-5-5-1-7 2-7 6 17 0 24 10 23 24-1 12-12 19-26 18l-9 5-1-7C9 49 5 41 7 28c2-11 12-17 24-17z',
};

/** Four original OpenLess companions; silhouettes are not third-party brand logos. */
export function AgentBuddy({
  color = 'mint',
  working = false,
  size = 42,
}: {
  color?: BuddyColor;
  working?: boolean;
  size?: number;
}) {
  const id = useId();
  const [light, base] = COLORS[color];
  return (
    <svg
      className={`agent-buddy${working ? ' is-working' : ''}`}
      viewBox="0 0 64 64"
      width={size}
      height={size}
      aria-hidden="true"
      style={{ '--buddy-color': base } as CSSProperties}
    >
      <defs>
        <linearGradient id={id} x1=".15" y1="0" x2=".8" y2="1">
          <stop stopColor={light} />
          <stop offset="1" stopColor={base} />
        </linearGradient>
      </defs>
      <path d={SHAPES[color]} fill={`url(#${id})`} />
      <path
        d="M17 23c2-5 5-7 10-7"
        fill="none"
        stroke="white"
        strokeOpacity=".4"
        strokeWidth="2.5"
        strokeLinecap="round"
      />
      <g className="agent-buddy-eyes" fill="#35372f">
        <ellipse cx="24" cy="31" rx="2.3" ry={color === 'violet' ? 2.4 : 3.1} />
        <ellipse cx="39" cy="31" rx="2.3" ry={color === 'violet' ? 2.4 : 3.1} />
      </g>
      <path
        d={color === 'amber' ? 'M28 39q4 5 8 0' : 'M29 40q3 2 6-1'}
        fill="none"
        stroke="#35372f"
        strokeWidth="2"
        strokeLinecap="round"
      />
      {color === 'coral' && (
        <g fill="#b86253" opacity=".35">
          <ellipse cx="18" cy="38" rx="3.5" ry="1.7" />
          <ellipse cx="46" cy="38" rx="3.5" ry="1.7" />
        </g>
      )}
      {color === 'violet' && (
        <path
          d="m26 13 6-3 6 3"
          fill="none"
          stroke="white"
          strokeOpacity=".5"
          strokeWidth="2"
          strokeLinecap="round"
        />
      )}
    </svg>
  );
}
