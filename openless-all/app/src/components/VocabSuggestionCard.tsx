// "Remember this word?" card, popping up at the bottom-right of the screen. A card
// instead of a queue in the vocab page because suggestions matter at the moment of
// correction; bottom-right because the capsule's centered spot covers the line being
// edited. Per-item accept/reject only, no bulk accept: automatic collection measured
// ~4 of 5 wrong on real devices, so eyeballing each one is the only reliable filter.

import { Icon } from './Icon';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { acceptPendingCorrection, rejectPendingCorrection } from '../lib/ipc';
import type { PendingCorrection } from '../lib/types';

interface VocabSuggestionCardProps {
  suggestions: PendingCorrection[];
}

export function VocabSuggestionCard({ suggestions }: VocabSuggestionCardProps) {
  const { t } = useTranslation();
  // Resolved items vanish from the card immediately — no waiting for a backend echo;
  // a click must react.
  const [resolved, setResolved] = useState<Set<string>>(new Set());
  const timerRef = useRef<number | null>(null);

  // Each suggestion has a Core deadline; new suggestions do not extend old ones.
  useEffect(() => {
    const active = suggestions.filter((suggestion) => !resolved.has(suggestion.id));
    if (active.length === 0) return;
    if (timerRef.current) clearTimeout(timerRef.current);
    const earliest = Math.min(...active.map((s) => s.expiresAtMs));
    timerRef.current = window.setTimeout(
      () => {
        const expired = active.filter((s) => s.expiresAtMs <= Date.now());
        setResolved((previous) => new Set([...previous, ...expired.map((s) => s.id)]));
        for (const suggestion of active) {
          if (suggestion.expiresAtMs <= Date.now()) {
            void rejectPendingCorrection(suggestion.id).catch(() => {});
          }
        }
      },
      Math.max(0, earliest - Date.now()),
    );
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, [suggestions, resolved]);

  const visible = suggestions.filter((s) => !resolved.has(s.id));
  if (visible.length === 0) return null;

  // Accept and reject share one optimistic update: hide locally first, restore on
  // failure so the user can retry.
  const resolve = async (id: string, commit: (id: string) => Promise<void>) => {
    setResolved((prev) => new Set(prev).add(id));
    try {
      await commit(id);
    } catch {
      setResolved((prev) => {
        const next = new Set(prev);
        next.delete(id);
        return next;
      });
    }
  };

  return (
    <div
      style={{
        width: '100%',
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        justifyContent: 'flex-end',
        padding: 12,
        // The card is the only thing taking mouse input — the capsule itself stays
        // pointerEvents:none at all times.
        pointerEvents: 'auto',
        boxSizing: 'border-box',
        animation: 'capsule-in .28s cubic-bezier(.3,1.1,.4,1) both',
      }}
    >
      <div
        style={{
          borderRadius: 16,
          padding: 12,
          background: 'var(--ol-capsule-pill-bg)',
          backdropFilter: 'blur(20px)',
          WebkitBackdropFilter: 'blur(20px)',
          // 1px solid border + spread shadow, same approach as the capsule itself.
          // Earlier `0.5px solid` fell on half a physical pixel, making the rounded
          // edges look blurry.
          border: '1px solid var(--ol-capsule-pill-border)',
          boxShadow: 'var(--ol-capsule-pill-shadow), var(--ol-capsule-pill-inset)',
          color: 'var(--ol-capsule-btn-ink)',
          fontFamily: 'var(--ol-font-sans)',
          // Children must never overflow the rounded corners.
          overflow: 'hidden',
        }}
      >
        <div
          style={{
            fontSize: 11,
            opacity: 0.55,
            marginBottom: 10,
            letterSpacing: 0.2,
          }}
        >
          {t('vocabCard.title')}
        </div>

        <div style={{ display: 'grid', gap: 8 }}>
          {visible.map((s) => (
            <div
              key={s.id}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 8,
                minWidth: 0,
                height: 28,
              }}
            >
              <span
                style={{
                  flex: 1,
                  minWidth: 0,
                  fontSize: 12.5,
                  fontFamily: 'var(--ol-font-mono)',
                  whiteSpace: 'nowrap',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                }}
                title={`${s.pattern} → ${s.replacement}`}
              >
                <span style={{ opacity: 0.4 }}>{s.pattern}</span>
                <span style={{ opacity: 0.3, margin: '0 5px' }}>→</span>
                {s.replacement}
              </span>
              <CardButton
                kind="accept"
                label={t('vocabCard.accept')}
                onClick={() => void resolve(s.id, acceptPendingCorrection)}
              />
              <CardButton
                kind="reject"
                label={t('vocabCard.reject')}
                onClick={() => void resolve(s.id, rejectPendingCorrection)}
              />
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

/// Accept/reject. Size, colors, and SVG paths copied from the capsule's confirm/cancel
/// pair — the same gesture in one product shouldn't look like two.
function CardButton({
  kind,
  label,
  onClick,
}: {
  kind: 'accept' | 'reject';
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      // The card floats over another app; pressing must not steal focus from where
      // the user is typing.
      onMouseDown={(event) => {
        event.preventDefault();
        event.stopPropagation();
      }}
      aria-label={label}
      title={label}
      style={{
        width: 28,
        height: 28,
        borderRadius: 999,
        flexShrink: 0,
        padding: 0,
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        background:
          kind === 'accept' ? 'var(--ol-capsule-btn-bg-confirm)' : 'var(--ol-capsule-btn-bg)',
        color: 'var(--ol-capsule-btn-ink)',
        border: '0.8px solid var(--ol-capsule-btn-border)',
        boxShadow: '0 1px 2px rgba(0, 0, 0, 0.06)',
        cursor: 'default',
        transition:
          'background 0.16s var(--ol-motion-quick), transform 0.12s var(--ol-motion-quick)',
      }}
    >
      <Icon name={kind === 'accept' ? 'check' : 'close'} size={13} strokeWidth={2.2} />
    </button>
  );
}
