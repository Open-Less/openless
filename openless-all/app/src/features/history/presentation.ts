import { useTranslation } from 'react-i18next';
import type { DictationSession, PolishMode, StylePack } from '../../api/contracts';
import { formatLocaleDecimal, formatLocaleNumber } from '../../lib/localeFormat';
import { packDisplayName } from '../../lib/history-repolish';

export function useModeLabel(): Record<PolishMode, string> {
  const { t } = useTranslation();
  return {
    raw: t('style.modes.raw.name'),
    light: t('style.modes.light.name'),
    structured: t('style.modes.structured.name'),
    formal: t('style.modes.formal.name'),
  };
}

// Pill defaults to nowrap + flexShrink: 0; a long pack name would squeeze the buttons on the same
// row out of shape (the "Copy" text wrapping vertically). Anywhere a pack name is shown, use a
// shrinkable + ellipsis style, with the full name in the outer container's title for hover.
export const TRUNCATED_PILL_STYLE = {
  minWidth: 0,
  maxWidth: '100%',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  display: 'block',
  flexShrink: 1,
} as const;

// Which style pack produced this text, shown on the history entry. session.mode is only the pack's
// baseMode (one of four built-in categories) — all custom packs fall into those buckets, so mode
// alone cannot identify the pack. Prefer resolving the real pack name via stylePackId, matching
// the style naming in this page's repolish panel. Built-in pack exceptions and naming go through
// packDisplayName; old history without stylePackId, or a deleted pack, falls back to the mode
// name too.
export function styleLabelFor(
  session: DictationSession,
  allPacks: StylePack[] | null,
  modeLabel: Record<PolishMode, string>,
): string {
  const pack = session.stylePackId
    ? allPacks?.find((candidate) => candidate.id === session.stylePackId)
    : undefined;
  return pack ? packDisplayName(pack, modeLabel) : modeLabel[session.mode];
}

export function historyTitle(
  session: DictationSession,
  t: ReturnType<typeof useTranslation>['t'],
): string {
  const text = (session.finalText || session.rawTranscript).trim();
  if (text) return text.split(/\r?\n/, 1)[0];
  if (session.errorCode === 'recording') return t('quickNote.recording', 'Recording…');
  if (session.errorCode === 'cancelled') {
    return t('quickNote.cancelledTitle', 'Recording cancelled');
  }
  if (session.errorCode) return t('quickNote.failedTitle', 'Recording needs attention');
  return t('quickNote.emptyTitle', 'Untitled recording');
}

/** Backend timeout errors degrade to bare strings at the IPC boundary (LLMError::Timeout →
 *  "timeout"). Only match whole-string common timeout shapes so other errors containing
 *  "timeout" are not misjudged as timeouts. */
export function isTimeout(message: string): boolean {
  const trimmed = message.trim();
  return /^(timeout|timed out|request timed out)$/i.test(trimmed) || trimmed.includes('超时');
}

export function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

export function isUserCancelled(message: string): boolean {
  const normalized = message.trim().toLowerCase();
  return (
    normalized === 'cancelled' ||
    normalized === 'canceled' ||
    normalized === 'user cancelled' ||
    normalized === 'user canceled'
  );
}

export function formatStepDuration(
  ms: number,
  t: ReturnType<typeof useTranslation>['t'],
  locale: string,
): string {
  if (ms < 1000)
    return t('common.durationMillis', { value: formatLocaleNumber(Math.round(ms), locale) });
  return formatDuration(ms, t, locale);
}

export function formatDuration(
  ms: number | null,
  t: ReturnType<typeof useTranslation>['t'],
  locale: string,
): string {
  if (ms == null || ms <= 0) return '—';
  const sec = ms / 1000;
  if (sec < 60) return t('common.durationSeconds', { value: formatLocaleDecimal(sec, locale) });
  return t('common.durationMinutes', { value: formatLocaleDecimal(sec / 60, locale) });
}
