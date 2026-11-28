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
        d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"
        fill={filled ? 'currentColor' : 'none'}
        stroke="currentColor"
        strokeWidth="1.7"
        strokeLinejoin="round"
      />
    </svg>
  );
}
