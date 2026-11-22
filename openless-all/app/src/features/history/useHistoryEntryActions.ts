import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { DictationSession } from '../../api/contracts';
import type { Interaction } from '../../api/interaction';
import { copyTextToClipboard, selectDirectory, shareFiles } from '../../api/files';
import {
  deleteHistoryEntry,
  exportAudioRecording,
  readAudioRecording,
  retranscribeRecording,
} from '../../api/history';
import { historyStore } from '../../data/history';
import { useHotkeySettings } from '../../data/preferences';
import { canRetranscribeHistoryEntry } from '../../lib/history-retranscribe';
import { getRuntimeOS } from '../../runtime/environment';
import { audioBytes, recordingFileName } from './audio';
import { errorMessage, historyTitle, isUserCancelled } from './presentation';

export function useHistoryEntryActions(
  item: DictationSession,
  quickNotesOnly: boolean,
  confirm: Interaction['confirm'],
  notify: Interaction['notify'],
) {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const [actionError, setActionError] = useState<string | null>(null);
  const [audioMissing, setAudioMissing] = useState(false);
  const [retranscribing, setRetranscribing] = useState(false);
  const [retranscriptionResult, setRetranscriptionResult] = useState<{
    sessionId: string;
    text: string;
  } | null>(null);
  const [justCopied, setJustCopied] = useState(false);
  const [justCopiedRaw, setJustCopiedRaw] = useState(false);
  const copyTimers = useRef(new Map<boolean, number>());
  const active = useRef(false);
  const writing = useRef(false);
  const transcribing = useRef(false);

  useEffect(() => {
    active.current = true;
    const timers = copyTimers.current;
    return () => {
      active.current = false;
      for (const timer of timers.values()) window.clearTimeout(timer);
      timers.clear();
    };
  }, []);

  const handleAudioMissing = useCallback(() => setAudioMissing(true), []);
  const reportAudioError = (
    error: unknown,
    translation: 'history.retranscribeFailed' | 'history.exportFailed',
  ) => {
    const message = errorMessage(error);
    if (isUserCancelled(message)) return;
    if (message.includes('not found')) {
      setAudioMissing(true);
      return;
    }
    setActionError(
      message === 'recording export failed'
        ? t('history.exportError')
        : t(translation, { err: message }),
    );
  };

  const onDelete = async () => {
    if (writing.current) return;
    writing.current = true;
    try {
      await confirm({
        title: t('common.delete'),
        description: t('history.confirmDeleteLocal'),
        confirmLabel: t('common.delete'),
        icon: 'trash',
        tone: 'danger',
        errorMessage: t('history.deleteFailed', { err: t('common.operationFailed') }),
        onConfirm: async () => {
          setActionError(null);
          await deleteHistoryEntry(item.id);
          await historyStore.refreshRecords();
          notify({
            message: t('history.deleted'),
            state: 'deleted',
          });
        },
      });
    } finally {
      writing.current = false;
    }
  };

  const onCopy = async (raw: boolean) => {
    const showCopied = raw ? setJustCopiedRaw : setJustCopied;
    try {
      const text = raw || !item.finalText.trim() ? item.rawTranscript : item.finalText;
      await copyTextToClipboard(text);
      if (!active.current) return;
      setActionError(null);
      showCopied(true);
      window.clearTimeout(copyTimers.current.get(raw));
      copyTimers.current.set(
        raw,
        window.setTimeout(() => {
          copyTimers.current.delete(raw);
          showCopied(false);
        }, 1500),
      );
    } catch (error) {
      setActionError(t('history.copyFailed', { err: errorMessage(error) }));
    }
  };

  const onExportAudio = async () => {
    if (!item.hasAudioRecording) return;
    try {
      await exportAudioRecording(item.id);
      setActionError(null);
    } catch (error) {
      reportAudioError(error, 'history.exportFailed');
    }
  };

  const onShareAudio = async () => {
    if (!item.hasAudioRecording) return;
    try {
      const bytes = audioBytes(await readAudioRecording(item.id));
      const file = new File([bytes], recordingFileName(item.id), { type: 'audio/wav' });
      if (!(await shareFiles(historyTitle(item, t), [file]))) await onExportAudio();
    } catch (error) {
      reportAudioError(error, 'history.exportFailed');
    }
  };

  const onChooseExportDirectory = async () => {
    if (!quickNotesOnly || !prefs || getRuntimeOS() === 'android') return;
    try {
      const picked = await selectDirectory({
        title: t('history.chooseSaveDirectory', 'Choose a transcript export directory'),
        prompt: {
          label: t('history.saveDirectoryPrompt', 'Enter a transcript export directory'),
          value: prefs.quickNoteExportDirectory,
        },
      });
      const directory = picked?.trim();
      if (!directory) return;
      await updatePrefs((current) => ({ ...current, quickNoteExportDirectory: directory }));
      setActionError(null);
    } catch (error) {
      setActionError(t('history.saveDirectoryFailed', { err: errorMessage(error) }));
    }
  };

  const onResetExportDirectory = async () => {
    if (!prefs?.quickNoteExportDirectory) return;
    try {
      await updatePrefs((current) => ({ ...current, quickNoteExportDirectory: '' }));
      setActionError(null);
    } catch (error) {
      setActionError(t('history.saveDirectoryFailed', { err: errorMessage(error) }));
    }
  };

  const onRetranscribe = async () => {
    if (!canRetranscribeHistoryEntry(item) || transcribing.current) return;
    transcribing.current = true;
    setRetranscribing(true);
    setRetranscriptionResult(null);
    setActionError(null);
    try {
      const result = await retranscribeRecording(item.id);
      if (!result.updatedEntry) setRetranscriptionResult({ sessionId: item.id, text: result.text });
    } catch (error) {
      reportAudioError(error, 'history.retranscribeFailed');
    } finally {
      transcribing.current = false;
      setRetranscribing(false);
    }
  };

  return {
    actionError,
    audioMissing,
    handleAudioMissing,
    retranscribing,
    retranscriptionResult,
    justCopied,
    justCopiedRaw,
    onCopy,
    onDelete,
    onExportAudio,
    onShareAudio,
    onRetranscribe,
    onChooseExportDirectory,
    onResetExportDirectory,
  };
}
