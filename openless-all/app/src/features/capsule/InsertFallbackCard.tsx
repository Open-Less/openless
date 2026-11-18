// Fallback card for "this text didn't make it in", shown at the bottom-right of the
// screen. Displays the FULL text (whatever landed on screen is only a partial), pauses
// its TTL while hovered, and copies via the backend: the button deliberately
// preventDefaults so it never steals focus, and navigator.clipboard throws
// "Document is not focused" on an unfocused document. No title — the text plus a Copy
// button is self-explanatory; payload.reason is logged only, never displayed.

import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { dismissInsertFallbackCard, reportInsertFallbackCardHeight } from '../../api/dictation';
import { copyTextToClipboard } from '../../api/files';
import {
  nextFallbackCardHeightReport,
  type FallbackCardHeightReport,
} from '../../lib/insertFallbackLayout';
import { useReducedMotion } from '../../lib/motion';
import type { InsertFallbackCardPayload } from '../../api/contracts';
import { isTauriHost } from '../../runtime/environment';
import { waitForViewportGeometry } from '../../runtime/windows/viewportGeometry';
import { useAppViewport } from '../../ui/layout/AppViewport';

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
  const { boundary } = useAppViewport();
  const native = isTauriHost();
  const reduced = useReducedMotion();
  const [layout, setLayout] = useState<'waiting' | 'ready' | 'bounded'>(
    native ? 'waiting' : 'ready',
  );
  const waiting = layout === 'waiting';
  const copyTimer = useRef<number>();
  const copyEpoch = useRef(0);
  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);
  const [paused, setPaused] = useState(false);
  const timerRef = useRef<number | null>(null);
  const cardRootRef = useRef<HTMLDivElement | null>(null);
  const lastHeightReportRef = useRef<FallbackCardHeightReport | null>(null);

  useLayoutEffect(() => {
    const element = cardRootRef.current;
    if (!native || !element) return;
    const viewport = boundary ?? document.documentElement;
    let disposed = false;
    let bounded = false;
    let request: AbortController | undefined;
    lastHeightReportRef.current = null;

    const reportHeight = () => {
      if (bounded) return;
      const report = nextFallbackCardHeightReport(
        lastHeightReportRef.current,
        payload.presentationId,
        element.getBoundingClientRect().height,
      );
      if (!report) return;
      lastHeightReportRef.current = report;
      request?.abort();
      const current = new AbortController();
      request = current;
      void reportInsertFallbackCardHeight(report.presentationId, report.height)
        .then(() =>
          waitForViewportGeometry(
            viewport,
            ({ height }) => height + 1 >= report.height,
            current.signal,
          ),
        )
        .then(
          () => {
            if (!disposed && !current.signal.aborted) setLayout('ready');
          },
          () => {
            if (!disposed && !current.signal.aborted) {
              // Preserve Copy and Dismiss within the actual space when the host
              // cannot fit every line. A constrained measurement must not
              // be reported as a new natural height and clear the bound.
              bounded = true;
              setLayout('bounded');
            }
          },
        );
    };

    reportHeight();
    const observer =
      typeof ResizeObserver === 'undefined' ? undefined : new ResizeObserver(reportHeight);
    observer?.observe(element);
    return () => {
      disposed = true;
      request?.abort();
      observer?.disconnect();
    };
  }, [boundary, native, payload.presentationId]);

  useEffect(
    () => () => {
      copyEpoch.current += 1;
      window.clearTimeout(copyTimer.current);
    },
    [],
  );

  // TTL countdown. Paused on hover: the cursor on the card means it's being read.
  useEffect(() => {
    if (paused || waiting) return;
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = window.setTimeout(() => {
      void dismissInsertFallbackCard();
    }, TTL_MS);
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, [waiting, paused, payload.presentationId]);

  const copy = async () => {
    const epoch = ++copyEpoch.current;
    window.clearTimeout(copyTimer.current);
    try {
      await copyTextToClipboard(payload.text);
      if (epoch !== copyEpoch.current) return;
      setCopied(true);
      setCopyFailed(false);
      copyTimer.current = window.setTimeout(() => setCopied(false), COPIED_FEEDBACK_MS);
    } catch {
      // Copy failure must be surfaced — this card is the last line of defense against
      // losing the text; failing silently again would leave the user no way to get it.
      if (epoch === copyEpoch.current) setCopyFailed(true);
    }
  };

  return (
    <div
      ref={cardRootRef}
      data-layout={layout}
      aria-hidden={waiting || undefined}
      style={{
        width: '100%',
        alignSelf: 'flex-end',
        display: 'flex',
        flexDirection: 'column',
        justifyContent: 'flex-end',
        padding: 12,
        // The card is the only thing taking mouse input — the capsule itself stays
        // pointerEvents:none at all times.
        pointerEvents: waiting ? 'none' : 'auto',
        visibility: waiting ? 'hidden' : 'visible',
        maxHeight: layout === 'bounded' ? '100%' : undefined,
        minHeight: 0,
        boxSizing: 'border-box',
      }}
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
    >
      <div
        style={{
          borderRadius: 'var(--ol-r-xl)',
          padding: 12,
          background: 'var(--ol-capsule-pill-bg)',
          backdropFilter: 'blur(20px)',
          WebkitBackdropFilter: 'blur(20px)',
          border: '1px solid var(--ol-capsule-pill-border)',
          boxShadow: 'var(--ol-capsule-pill-shadow), var(--ol-capsule-pill-inset)',
          color: 'var(--ol-capsule-btn-ink)',
          fontFamily: 'var(--ol-font-sans)',
          overflow: 'hidden',
          display: 'flex',
          flexDirection: 'column',
          minHeight: 0,
          animation: waiting || reduced ? 'none' : 'capsule-in .28s cubic-bezier(.3,1.1,.4,1) both',
        }}
      >
        <div
          style={{
            fontSize: 13,
            lineHeight: `${LINE_HEIGHT}px`,
            minHeight: 0,
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

        <div style={{ display: 'flex', flexShrink: 0, alignItems: 'center', gap: 8 }}>
          <TextButton
            primary
            disabled={waiting}
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
            disabled={waiting}
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
  disabled,
  onClick,
}: {
  label: string;
  primary?: boolean;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <button
      disabled={disabled}
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
        borderRadius: 'var(--ol-radius)',
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
