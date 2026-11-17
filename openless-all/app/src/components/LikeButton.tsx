import { useState } from 'react';

interface LikeButtonProps {
  liked: boolean;
  count: number;
  label: string;
  compact?: boolean;
  onClick: () => void;
}

/** Heart that pops once and blooms when a like lands. The control stays mounted. */
export function LikeButton({ liked, count, label, compact = false, onClick }: LikeButtonProps) {
  const [pulse, setPulse] = useState(0);
  return (
    <button
      type="button"
      className={compact ? 'ol-like ol-like-compact' : 'ol-like'}
      data-on={liked ? 'true' : undefined}
      aria-pressed={liked}
      aria-label={label}
      onClick={(event) => {
        event.stopPropagation();
        if (!liked) setPulse((current) => current + 1);
        onClick();
      }}
    >
      <span className="ol-like-heart">
        <span
          key={pulse}
          className="ol-like-glyph"
          data-on={liked ? 'true' : undefined}
          data-pop={pulse > 0 ? 'true' : undefined}
        >
          <HeartGlyph filled={liked} />
        </span>
        {pulse > 0 && liked && <span key={`bloom-${pulse}`} className="ol-like-bloom" />}
      </span>
      <span className="ol-like-count">{count}</span>
    </button>
  );
}

function HeartGlyph({ filled }: { filled: boolean }) {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" aria-hidden="true">
      <path
        d="M12 20.4s-6.8-4.15-8.7-8.15C1.7 9.2 2.8 6.2 5.8 5.45c1.9-.48 3.6.35 4.7 1.85 1.1-1.5 2.8-2.33 4.7-1.85 3 .75 4.1 3.75 2.5 6.8-1.9 4-8.7 8.15-8.7 8.15z"
        fill={filled ? 'currentColor' : 'none'}
        stroke="currentColor"
        strokeWidth="1.7"
        strokeLinejoin="round"
      />
    </svg>
  );
}
