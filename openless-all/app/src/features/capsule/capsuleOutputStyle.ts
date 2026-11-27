import type {
  CapsulePayload,
  CapsuleStyleMenuKey,
  CapsuleStyleMenuRequest,
  DictationStyleTarget,
  PolishMode,
} from '../../api/contracts';
import { stylePackDisplayName } from '../../lib/stylePackPresentation';

export interface CapsuleOutputStyle {
  packId: string | null;
  name: string | null;
}

export function sameCapsuleOutputStyle(
  current: CapsuleOutputStyle | null,
  next: CapsuleOutputStyle | null,
) {
  return (
    current === next ||
    (current !== null &&
      next !== null &&
      current.packId === next.packId &&
      current.name === next.name)
  );
}

export function projectCapsuleOutputStyle(
  frame: Pick<
    CapsulePayload,
    | 'state'
    | 'qa'
    | 'selectionPolish'
    | 'operating'
    | 'skipAutoStyle'
    | 'stylePackId'
    | 'stylePackName'
  >,
): CapsuleOutputStyle | null {
  const capturing = frame.state === 'recording';
  if (
    !frame.skipAutoStyle ||
    frame.qa ||
    frame.selectionPolish ||
    frame.operating ||
    !(capturing || frame.state === 'transcribing' || frame.state === 'polishing')
  )
    return null;
  return {
    packId: frame.stylePackId ?? null,
    name: frame.stylePackName?.trim() || null,
  };
}

export function capsuleOutputStyleLabel(
  style: CapsuleOutputStyle,
  labels: Record<PolishMode, string>,
) {
  const mode = (['raw', 'light', 'structured', 'formal'] as const).find(
    (mode) => style.packId === `builtin.${mode}`,
  );
  if (!mode) return style.name;
  if (!style.name) return labels[mode];
  return stylePackDisplayName(
    { id: `builtin.${mode}`, kind: 'builtin', baseMode: mode, name: style.name },
    labels,
  );
}

export function stylePickerRequestForSession(
  request: CapsuleStyleMenuRequest | null,
  captureSessionId: string | undefined,
  enabled: boolean,
) {
  return enabled && request?.sessionId === captureSessionId ? request : null;
}

export function forgetStylePickerRequest<T extends CapsuleStyleMenuRequest>(
  current: T | null,
  stale: CapsuleStyleMenuRequest,
): T | null {
  return current?.sessionId === stale.sessionId && current.requestId === stale.requestId
    ? null
    : current;
}

export class CapsuleStylePickerRequests {
  private sessionId: string | undefined;
  private consumed = new Set<string>();

  hasConsumed(intent: CapsuleStyleMenuRequest) {
    return this.sessionId === intent.sessionId && this.consumed.has(intent.requestId);
  }

  take(intent: CapsuleStyleMenuRequest, target: DictationStyleTarget) {
    if (
      this.hasConsumed(intent) ||
      intent.sessionId !== target.sessionId ||
      !target.mutable ||
      target.selection.mode !== 'skipAuto'
    )
      return null;
    const styles = target.availableStyles;
    const initial =
      styles.find(({ id }) => id === target.polishStylePackId) ??
      styles.find(({ id }) => id === target.stylePackId) ??
      styles[0];
    if (!initial) return null;
    if (this.sessionId !== intent.sessionId) {
      this.sessionId = intent.sessionId;
      this.consumed.clear();
    }
    this.consumed.add(intent.requestId);
    return initial.id;
  }
}

export type StyleMenuKeyAction =
  { type: 'close' } | { type: 'highlight' | 'select'; stylePackId: string };

export function styleMenuKeyAction(
  { key, repeat }: Pick<CapsuleStyleMenuKey, 'key' | 'repeat'>,
  availableStyles: ReadonlyArray<{ id: string }>,
  highlightedId: string,
): StyleMenuKeyAction | undefined {
  if ((key === 'Escape' || key === 'Enter') && repeat) return;
  if (key === 'Escape') return { type: 'close' };
  const index = availableStyles.findIndex(({ id }) => id === highlightedId);
  if (index < 0) return;
  if (key === 'Enter') return { type: 'select', stylePackId: highlightedId };
  if (key !== 'ArrowUp' && key !== 'ArrowDown') return;
  const step = key === 'ArrowUp' ? -1 : 1;
  const next = (index + step + availableStyles.length) % availableStyles.length;
  return { type: 'highlight', stylePackId: availableStyles[next].id };
}
