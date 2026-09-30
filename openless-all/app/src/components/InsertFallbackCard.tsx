// Fallback card for "this text didn't make it in", shown at the bottom-right of the
// screen. Displays the FULL text (whatever landed on screen is only a partial), pauses
// its TTL while hovered, and copies via the backend: the button deliberately
// preventDefaults so it never steals focus, and navigator.clipboard throws
// "Document is not focused" on an unfocused document. No title — the text plus a Copy
// button is self-explanatory; payload.reason is logged only, never displayed.

import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  copyTextToClipboard,
  dismissInsertFallbackCard,
  reportInsertFallbackCardHeight,
} from '../lib/ipc';
import {
  nextFallbackCardHeightReport,
  type FallbackCardHeightReport,
} from '../lib/insertFallbackLayout';
import type { InsertFallbackCardPayload } from '../lib/types';

/// Auto-dismiss delay. Double the vocab card's 10s — this one needs reading.
const TTL_MS = 20_000;
/// How long the button stays on "Copied" after a successful copy.
const COPIED_FEEDBACK_MS = 1_600;
/// Max body lines before scrolling inside the card.
/// Native windows measure real DOM height; these layout constants are no longer
/// maintained in parallel.
const MAX_LINES = 8;
const LINE_HEIGHT = 18;

interface InsertFallbackCardProps {
  payload: InsertFallbackCardPayload;
}

export function InsertFallbackCard({ payload }: InsertFallbackCardProps) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);
  const [paused, setPaused] = useState(false);
  const timerRef = useRef<number | null>(null);
  const cardRootRef = useRef<HTMLDivElement | null>(null);
  const lastHeightReportRef = useRef<FallbackCardHeightReport | null>(null);

  useLayoutEffect(() => {
    const element = cardRootRef.current;
    if (!element) return undefined;
    let cancelled = false;

    const reportHeight = () => {
      const report = nextFallbackCardHeightReport(
        lastHeightReportRef.current,
        payload.presentationId,
        element.getBoundingClientRect().height,
      );
      if (!report) return;
      lastHeightReportRef.current = report;
      void reportInsertFallbackCardHeight(report.presentationId, report.height).catch(() => {
        // On transient IPC failure, clear the report so a later ResizeObserver tick retries.
        if (
          !cancelled &&
          lastHeightReportRef.current?.presentationId === report.presentationId &&
          lastHeightReportRef.current.height === report.height
        ) {
          lastHeightReportRef.current = null;
        }
      });
    };

    lastHeightReportRef.current = null;
    reportHeight();
    if (typeof ResizeObserver === 'undefined') return undefined;
    const observer = new ResizeObserver(reportHeight);
    observer.observe(element);
    return () => {
      cancelled = true;
      observer.disconnect();
    };
  }, [payload.presentationId]);

  // TTL countdown. Paused on hover: the cursor on the card means it's being read.
  useEffect(() => {
    if (paused) return;
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = window.setTimeout(() => {
      void dismissInsertFallbackCard();
    }, TTL_MS);
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, [paused, payload.text]);

  const copy = async () => {
    try {
      await copyTextToClipboard(payload.text);
      setCopied(true);
      setCopyFailed(false);
      window.setTimeout(() => setCopied(false), COPIED_FEEDBACK_MS);
    } catch {
      // Copy failure must be surfaced — this card is the last line of defense against
      // losing the text; failing silently again would leave the user no way to get it.
      setCopyFailed(true);
    }
  };

  return (
    <div
      ref={cardRootRef}
      style={{
        width: '100%',
        alignSelf: 'flex-end',
        display: 'flex',
        flexDirection: 'column',
        justifyContent: 'flex-end',
        padding: 12,
        // The card is the only thing taking mouse input — the capsule itself stays
        // pointerEvents:none at all times.
        pointerEvents: 'auto',
        boxSizing: 'border-box',
      }}
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
    >
      <div
        style={{
          borderRadius: 16,
          padding: 12,
          background: 'var(--ol-capsule-pill-bg)',
          backdropFilter: 'blur(20px)',
          WebkitBackdropFilter: 'blur(20px)',
          border: '1px solid var(--ol-capsule-pill-border)',
          boxShadow: 'var(--ol-capsule-pill-shadow), var(--ol-capsule-pill-inset)',
          color: 'var(--ol-capsule-btn-ink)',
          fontFamily: 'var(--ol-font-sans)',
          overflow: 'hidden',
          animation: 'capsule-in .28s cubic-bezier(.3,1.1,.4,1) both',
        }}
      >
        <div
          style={{
            fontSize: 13,
            lineHeight: `${LINE_HEIGHT}px`,
            maxHeight: LINE_HEIGHT * MAX_LINES,
            overflowY: 'auto',
            // Allow manual selection — some users only want one sentence.
            userSelect: 'text',
            cursor: 'text',
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            marginBottom: 10,
          }}
        >
          {payload.text}
        </div>

        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <TextButton
            primary
            label={
              copyFailed
                ? t('insertFallbackCard.copyFailed')
                : copied
                  ? t('insertFallbackCard.copied')
                  : t('insertFallbackCard.copy')
            }
            onClick={() => void copy()}
          />
          <TextButton
            label={t('insertFallbackCard.dismiss')}
            onClick={() => void dismissInsertFallbackCard()}
          />
        </div>
      </div>
    </div>
  );
}

/// Buttons reuse the capsule's confirm/cancel palette; a labeled wide button because
/// "Copy" needs words, not just an icon.
function TextButton({
  label,
  primary,
  onClick,
}: {
  label: string;
  primary?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      // The card floats over another app; pressing must not steal focus from where the
      // user is typing. That's also why copy goes through the backend
      // (navigator.clipboard requires a focused document).
      onMouseDown={(event) => {
        event.preventDefault();
        event.stopPropagation();
      }}
      style={{
        flex: primary ? 1 : undefined,
        height: 28,
        padding: '0 14px',
        borderRadius: 999,
        fontSize: 12,
        fontFamily: 'var(--ol-font-sans)',
        background: primary ? 'var(--ol-capsule-btn-bg-confirm)' : 'var(--ol-capsule-btn-bg)',
        color: 'var(--ol-capsule-btn-ink)',
        border: '0.8px solid var(--ol-capsule-btn-border)',
        boxShadow: '0 1px 2px rgba(0, 0, 0, 0.06)',
        cursor: 'default',
        transition:
          'background 0.16s var(--ol-motion-quick), transform 0.12s var(--ol-motion-quick)',
      }}
    >
      {label}
    </button>
  );
}
