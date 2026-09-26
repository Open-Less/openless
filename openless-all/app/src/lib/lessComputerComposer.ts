import type { HotkeyMode } from './types';

const CJK = /[\u2e80-\u303f\u3040-\u9fff\uac00-\ud7af\uf900-\ufaff\uff00-\uffef]/;
const CLOSING_PUNCTUATION = /^[,.!?;:%)\]}，。！？；：、）」』》]/;

/** Append a finished dictation to the draft; CJK text and punctuation never get an extra space. */
export function mergeDictation(draft: string, transcript: string): string {
  const addition = transcript.trim();
  if (!addition) return draft;
  if (!draft.trim()) return addition;
  const last = draft[draft.length - 1];
  if (
    /\s/.test(last) ||
    CJK.test(last) ||
    CJK.test(addition[0]) ||
    CLOSING_PUNCTUATION.test(addition)
  )
    return draft + addition;
  return `${draft} ${addition}`;
}

/** Live transcript line: keep the most recent words visible inside a single composer row. */
export function transcriptTail(text: string, maxChars = 96): string {
  const normalized = text.replace(/\s+/g, ' ').trim();
  const chars = Array.from(normalized);
  return chars.length <= maxChars ? normalized : `…${chars.slice(-maxChars).join('')}`;
}

export type VoiceHintKey = 'holdHint' | 'toggleHint' | 'autoHint';

/** Less Computer shares the dictation hotkey mode; double-click behaves like a single toggle press. */
export function voiceHintKey(mode: HotkeyMode): VoiceHintKey {
  if (mode === 'hold') return 'holdHint';
  if (mode === 'auto') return 'autoHint';
  return 'toggleHint';
}

const CLAIM_KEY = 'ol.lc.dictation-claims';
const MAX_CLAIMS = 32;
let claims: string[] | null = null;

function loadClaims(): string[] {
  if (claims) return claims;
  try {
    const parsed: unknown = JSON.parse(globalThis.sessionStorage?.getItem(CLAIM_KEY) ?? '[]');
    claims = Array.isArray(parsed)
      ? parsed.filter((id): id is string => typeof id === 'string')
      : [];
  } catch {
    claims = [];
  }
  return claims;
}

/**
 * The idle projection of a finished dictation is replayed on remount and after
 * a WebView reload. Only the first observer may apply it to the draft.
 */
export function claimDictationResult(sessionId: string): boolean {
  const list = loadClaims();
  if (list.includes(sessionId)) return false;
  list.push(sessionId);
  if (list.length > MAX_CLAIMS) list.splice(0, list.length - MAX_CLAIMS);
  try {
    globalThis.sessionStorage?.setItem(CLAIM_KEY, JSON.stringify(list));
  } catch {
    /* in-memory claims still prevent duplicates for this WebView */
  }
  return true;
}
