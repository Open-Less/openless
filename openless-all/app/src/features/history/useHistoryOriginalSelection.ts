import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { DictationSession } from '../../api/contracts';
import { copyTextToClipboard } from '../../api/files';
import type { Interaction } from '../../api/interaction';
import { errorMessage } from './presentation';
import { hasSelectionOriginal, selectionOriginalClipboardText } from './selectionOriginals';

export function useHistoryOriginalSelection(
  entries: DictationSession[],
  notify: Interaction['notify'],
) {
  const { t } = useTranslation();
  const [selection, setSelection] = useState<Set<string> | null>(null);
  const [copying, setCopying] = useState(false);
  const inFlight = useRef(false);
  const availableIds = useMemo(
    () => new Set(entries.filter(hasSelectionOriginal).map(({ id }) => id)),
    [entries],
  );
  const selectedIds = useMemo(
    () => new Set(Array.from(selection ?? []).filter((id) => availableIds.has(id))),
    [selection, availableIds],
  );

  useEffect(() => {
    // Filtering or deleting a row must not leave its original selected off-screen.
    setSelection((current) => {
      if (!current?.size) return current;
      const next = new Set(Array.from(current).filter((id) => availableIds.has(id)));
      return current.size === next.size ? current : next;
    });
  }, [availableIds]);

  const toggleEntry = useCallback((id: string) => {
    setSelection((current) => {
      if (!current) return current;
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const copy = async () => {
    if (inFlight.current || selectedIds.size === 0) return;
    const text = selectionOriginalClipboardText(entries, selectedIds);
    if (text.length === 0) return;
    inFlight.current = true;
    setCopying(true);
    try {
      await copyTextToClipboard(text);
      notify({ state: 'saved', message: t('common.copied') });
    } catch (error) {
      notify({
        state: 'failed',
        message: t('history.copyFailed', { err: errorMessage(error) }),
      });
    } finally {
      inFlight.current = false;
      setCopying(false);
    }
  };

  return {
    active: selection !== null,
    availableIds,
    selectedIds,
    copying,
    toggleMode: () => setSelection((current) => (current === null ? new Set<string>() : null)),
    selectAll: () => setSelection(new Set(availableIds)),
    toggleEntry,
    copy,
  };
}
