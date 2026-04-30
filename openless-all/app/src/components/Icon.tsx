// Icon.tsx — minimal stroke icons (1.5 stroke). Matches the black/blue aesthetic.
// Usage: <Icon name="overview" size={16} />

import type { CSSProperties } from 'react';

export const ICONS: Record<string, string> = {
  overview: 'M3 13l4-4 3 3 7-7M14 5h4v4',
  history: 'M3 12a9 9 0 1 0 3-6.7M3 4v4h4',
  vocab:   'M5 4h11a2 2 0 0 1 2 2v13l-3-2-3 2-3-2-3 2V6a2 2 0 0 1 2-2zM8 9h7M8 13h5',
  style:   'M12 3a9 9 0 1 0 0 18 3 3 0 0 0 3-3v-1a2 2 0 0 1 2-2h1a3 3 0 0 0 3-3 9 9 0 0 0-9-9z',
  settings:'M12 9.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1A2 2 0 1 1 7 4.9l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z',
  help:    'M9.1 9a3 3 0 0 1 5.8 1c0 2-3 3-3 3M12 17h.01M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0z',
  mic:     'M12 2a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3zM19 11a7 7 0 0 1-14 0M12 18v3M8 21h8',
  search:  'M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM21 21l-4.5-4.5',
  plus:    'M12 5v14M5 12h14',
  check:   'M5 12l4 4 10-10',
  x:       'M6 6l12 12M6 18L18 6',
  copy:    'M9 9h10v10H9zM5 15V5h10',
  eye:     'M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12zM12 9.5a2.5 2.5 0 1 1 0 5 2.5 2.5 0 0 1 0-5z',
  trash:   'M4 7h16M9 7V4h6v3M6 7v13a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V7M10 11v7M14 11v7',
  refresh: 'M4 4v6h6M20 20v-6h-6M4 10a8 8 0 0 1 14-3l2 3M20 14a8 8 0 0 1-14 3l-2-3',
  sparkle: 'M12 3v3M12 18v3M5 12H2M22 12h-3M6 6l-2-2M20 20l-2-2M6 18l-2 2M20 4l-2 2M12 8a4 4 0 0 0 4 4 4 4 0 0 0-4 4 4 4 0 0 0-4-4 4 4 0 0 0 4-4z',
  bolt:    'M13 2L4 14h7l-1 8 9-12h-7l1-8z',
  clock:   'M12 7v5l3 2M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0z',
  hash:    'M5 9h14M5 15h14M10 3l-2 18M16 3l-2 18',
  chevDown:'M6 9l6 6 6-6',
  chevRight:'M9 6l6 6-6 6',
  chevLeft:'M15 6l-6 6 6 6',
  chevLR:  'M8 5l-3 7 3 7M16 5l3 7-3 7',
  collapse:'M9 4h11v16H9M14 9l-3 3 3 3M4 4v16',
  expand:  'M4 4h16v16H4zM10 9l-3 3 3 3M14 9l3 3-3 3',
  layout:  'M3 4h18v6H3zM3 14h7v6H3zM14 14h7v6h-7z',
  cmd:     'M9 6a3 3 0 1 0 0 6h6a3 3 0 1 0 0-6 3 3 0 0 0-3 3v6a3 3 0 1 0 3-3H9a3 3 0 1 0 3 3z',
  option:  'M5 6h4l5 12h5M14 6h5',
  esc:     'M3 7h18v10H3zM7 10l3 4M7 14l3-4M14 10v4M14 14h3M14 10h3M14 12h3',
  enter:   'M21 7v4a3 3 0 0 1-3 3H5M9 18l-4-4 4-4',
  inserted:'M5 12l4 4 10-10',
  cloud:   'M7 18h11a4 4 0 0 0 .5-8A6 6 0 0 0 7 11a4 4 0 0 0 0 7z',
  mac:     'M16 4a4 4 0 0 0-4 4 4 4 0 0 0-4-4C5 4 3 7 3 11s2 9 5 9c1.5 0 2-1 4-1s2.5 1 4 1c3 0 5-5 5-9s-2-7-5-7zM13 4c0-1 1-2 2-2',
  win:     'M3 5l8-1v8H3zM12 4l9-1v9h-9zM3 13h8v8l-8-1zM12 13h9v8l-9-1z',
  doc:     'M6 3h8l5 5v13H6zM14 3v5h5',
  link:    'M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 1 0-5.7-5.7L11 7M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 1 0 5.7 5.7L13 17',
  filter:  'M3 5h18l-7 9v6l-4-2v-4z',
  archive: 'M3 4h18v4H3zM5 8v12h14V8M9 12h6',
  tag:     'M3 11V3h8l10 10-8 8L3 11zM7 7h.01',
  user:    'M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4 21a8 8 0 0 1 16 0',
  mail:    'M3 6h18v12H3zM3 6l9 7 9-7',
  info:    'M12 8h.01M11 12h1v4h1M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0z',
  external:'M9 5h10v10M19 5L9 15M5 9v10h10',
  close:   'M6 6l12 12M6 18L18 6',
};

export interface IconProps {
  name: string;
  size?: number;
  stroke?: string;
  strokeWidth?: number;
  fill?: string;
  style?: CSSProperties;
  className?: string;
}

export function Icon({
  name,
  size = 16,
  stroke = 'currentColor',
  strokeWidth = 1.5,
  fill = 'none',
  style,
  className,
}: IconProps) {
  const d = ICONS[name];
  if (!d) return null;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill={fill}
      stroke={stroke}
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      style={style}
      className={className}
      aria-hidden="true"
    >
      <path d={d} />
    </svg>
  );
}
