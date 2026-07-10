// Icon.tsx — minimal stroke icons (1.5 stroke). Matches the black/blue aesthetic.
// Usage: <Icon name="overview" size={16} />

import type { CSSProperties } from 'react';

export const ICONS: Record<string, string> = {
  overview: 'M3 3v18h18M18 17V9M13 17V5M8 17v-3', // Simplified bar chart
  history: 'M12 8v4l3 3m6-3a9 9 0 1 1-18 0 9 9 0 0 1 18 0z', // Clock
  vocab: 'M4 19.5A2.5 2.5 0 0 1 6.5 17H20M4 19.5A2.5 2.5 0 0 0 6.5 22H20V4H6.5A2.5 2.5 0 0 0 4 6.5v13Z', // Book
  style: 'M12 2v20M17 5H9.5a3.5 3.5 0 0 0 0 7h5a3.5 3.5 0 0 1 0 7H6', // Type / Font
  translate: 'M12 2a10 10 0 1 0 0 20 10 10 0 1 0 0-20zM2 12h20M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z', // Globe
  selectionAsk: 'M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z', // Message square
  settings: 'M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0z', // Settings gear
  help: 'M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3M12 17h.01M12 22c5.523 0 10-4.477 10-10S17.523 2 12 2 2 6.477 2 12s4.477 10 10 10z', // Help circle
  mic: 'M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3zM19 10v2a7 7 0 0 1-14 0v-2M12 19v4M8 23h8', // Mic
  search: 'M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16zM21 21l-4.35-4.35', // Search
  plus: 'M12 5v14M5 12h14', // Plus
  check: 'M20 6L9 17l-5-5', // Check
  x: 'M18 6L6 18M6 6l12 12', // X
  copy: 'M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2M15 2H9a1 1 0 0 0-1 1v2a1 1 0 0 0 1 1h6a1 1 0 0 0 1-1V3a1 1 0 0 0-1-1z', // Clipboard
  eye: 'M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z', // Eye
  trash: 'M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M10 11v6M14 11v6', // Trash
  refresh: 'M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8M3 3v5h5', // Refresh ccw
  sparkle: 'M12 3L14.5 9.5 21 12 14.5 14.5 12 21 9.5 14.5 3 12 9.5 9.5 12 3Z', // Sparkle
  bolt: 'M13 2L3 14h9l-1 8 10-12h-9l1-8z', // Zap
  clock: 'M12 8v4l3 3M12 22a10 10 0 1 1 0-20 10 10 0 0 1 0 20z', // Clock standard
  hash: 'M4 9h16M4 15h16M10 3L8 21M16 3l-2 18', // Hash
  chevDown: 'M6 9l6 6 6-6', // Chevron down
  chevRight: 'M9 18l6-6-6-6', // Chevron right
  chevLeft: 'M15 18l-6-6 6-6', // Chevron left
  chevLR: 'M8 5L5 12l3 7M16 5l3 7-3 7', // Code
  collapse: 'M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7', // Shrink
  expand: 'M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7', // Expand
  layout: 'M3 3h18v18H3zM3 9h18M9 21V9', // Layout (Sidebar)
  cmd: 'M18 3a3 3 0 0 0-3 3v12a3 3 0 0 0 3 3 3 3 0 0 0 3-3 3 3 0 0 0-3-3H6a3 3 0 0 0-3 3 3 3 0 0 0 3 3 3 3 0 0 0 3-3V6a3 3 0 0 0-3-3 3 3 0 0 0-3 3 3 3 0 0 0 3 3h12a3 3 0 0 0 3-3 3 3 0 0 0-3-3z', // Command
  option: 'M3 3h6l6 18h6M14 3h7', // Option
  esc: 'M10 6l-6 6 6 6M4 12h16', // Move left (escape substitute)
  enter: 'M9 10L4 15l5 5M20 4v7a4 4 0 0 1-4 4H4', // Corner down left
  inserted: 'M20 6L9 17l-5-5', // Check
  cloud: 'M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9z', // Cloud
  mac: 'M4 6a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6zm8 14v2m-4 0h8', // Monitor
  win: 'M4 14.5V20l16-2v-3.5H4zm16-4V2l-16 2v4.5h16z', // Windows logo proxy (Squares)
  doc: 'M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8zM14 2v6h6', // File
  link: 'M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71', // Link
  filter: 'M22 3H2l8 9.46V19l4 2v-8.54L22 3z', // Filter
  archive: 'M21 8v13H3V8M1 3h22v5H1zM10 12h4', // Archive
  tag: 'M20.59 13.41l-7.17 7.17a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82zM7 7h.01', // Tag
  user: 'M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2M12 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8z', // User
  mail: 'M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4c-1.1 0-2-.9-2-2V6c0-1.1.9-2 2-2zM22 6l-10 7L2 6', // Mail
  info: 'M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20zM12 16v-4M12 8h.01', // Info
  shield: 'M12 22s8-3 8-9V5l-8-3-8 3v8c0 6 8 9 8 9z', // Shield (privacy)
  external: 'M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6M15 3h6v6M10 14L21 3', // External link
  close: 'M18 6L6 18M6 6l12 12', // Close / X
  more: 'M5 12h.01M12 12h.01M19 12h.01', // More (horizontal dots)
  play: 'M5 3l14 9-14 9V3z', // Play
  download: 'M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M7 10l5 5 5-5M12 15V3', // Download
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
  strokeWidth = 1.8,
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
