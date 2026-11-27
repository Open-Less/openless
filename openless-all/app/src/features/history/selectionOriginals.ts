import type { DictationSession } from '../../api/contracts';

type SelectionHistoryEntry = Pick<DictationSession, 'id' | 'source' | 'rawTranscript'>;

export function hasSelectionOriginal(entry: Omit<SelectionHistoryEntry, 'id'>): boolean {
  return (
    entry.source === 'selection_polish' &&
    typeof entry.rawTranscript === 'string' &&
    entry.rawTranscript.length > 0
  );
}

/** Preserve each original verbatim and combine them in displayed list order. */
export function selectionOriginalClipboardText(
  entries: readonly SelectionHistoryEntry[],
  selectedIds: ReadonlySet<string>,
): string {
  return entries
    .filter((entry) => selectedIds.has(entry.id) && hasSelectionOriginal(entry))
    .map((entry) => entry.rawTranscript)
    .join('\n\n');
}
