import { MicIcon, PencilLineIcon, QuoteIcon } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { chatPanelFocusKeyboard, stopQaDraftRecording } from '../../api/qa';
import { QaPromptBox } from './QaPromptBox';
import { useQaDraftDictation } from './useQaDraftDictation';
import { QaVoiceStrip } from './QaVoiceStrip';

export type QaStatus = 'idle' | 'recording' | 'thinking' | 'error';

interface QaComposerProps {
  value: string;
  status: QaStatus;
  level: number;
  voiceActive: boolean;
  selectionPreview: string;
  busy: boolean;
  micBusy: boolean;
  embedded: boolean;
  available: boolean;
  draftDictationEnabled: boolean;
  contentScale: number;
  editInstructionMode: boolean;
  onEditInstructionModeChange: (enabled: boolean) => void;
  onChange: (value: string) => void;
  onSubmit: () => void;
  onToggleRecording: () => void;
  onCancelTurn: () => void;
  cancelBusy: boolean;
}

export function QaComposer({
  value,
  status,
  level,
  voiceActive,
  selectionPreview,
  busy,
  micBusy,
  embedded,
  available,
  draftDictationEnabled,
  contentScale,
  editInstructionMode,
  onEditInstructionModeChange,
  onChange,
  onSubmit,
  onToggleRecording,
  onCancelTurn,
  cancelBusy,
}: QaComposerProps) {
  const { t } = useTranslation();
  const recording = status === 'recording';
  const generating = status === 'thinking';
  const voiceLabel = t('qa.voiceInputPlaceholder');
  const inputLabel = t('qa.composerPlaceholder');
  const editor = useQaDraftDictation(draftDictationEnabled, onChange);
  const draft = editor.recording;
  const stopLabel = t('qa.micStop');

  return (
    <QaPromptBox
      value={value}
      label={inputLabel}
      placeholder={voiceActive ? voiceLabel : inputLabel}
      disabled={!available}
      readOnly={voiceActive}
      recording={recording ? 'question' : draft ? 'draft' : undefined}
      recordingControl={
        recording || draft ? (
          <>
            <QaVoiceStrip
              level={recording ? level : (draft?.level ?? 0)}
              elapsedMs={recording ? undefined : draft?.elapsedMs}
              contentScale={contentScale}
              label={voiceLabel}
              stopLabel={stopLabel}
              disabled={!available || (recording && micBusy)}
              onStop={() => {
                if (recording) onToggleRecording();
                else if (draft) void stopQaDraftRecording(draft.sessionId).catch(() => undefined);
              }}
            />
            {recording && selectionPreview && (
              <span className="qa-recording-selection" title={selectionPreview}>
                <QuoteIcon />
                <span>{selectionPreview}</span>
              </span>
            )}
          </>
        ) : undefined
      }
      sendDisabled={busy || !!draft}
      sendLabel={t('qa.composerSend')}
      stop={
        generating
          ? { label: t('qa.stop'), disabled: !available || cancelBusy, onClick: onCancelTurn }
          : undefined
      }
      onChange={onChange}
      editorRef={editor.input}
      onEditorFocusChange={editor.setFocused}
      onSubmit={onSubmit}
      onRequestKeyboard={() => {
        if (available && !embedded) void chatPanelFocusKeyboard().catch(() => undefined);
      }}
      actions={
        <>
          <button
            className={`qa-mode${editInstructionMode ? ' is-on' : ''}`}
            type="button"
            aria-pressed={editInstructionMode}
            disabled={!available || busy}
            onClick={() => onEditInstructionModeChange(!editInstructionMode)}
            aria-label={t('qa.editInstructionMode')}
            title={t('qa.editInstructionMode')}
          >
            <PencilLineIcon />
          </button>
          <button
            className="qa-mic"
            type="button"
            disabled={!available || generating || micBusy || busy}
            onClick={onToggleRecording}
            aria-label={t('qa.micLabel')}
            title={t('qa.micLabel')}
          >
            <MicIcon />
          </button>
        </>
      }
      feedback={
        !available ? (
          <span className="qa-browser-note">{t('qa.compact.browserUnavailable')}</span>
        ) : undefined
      }
    />
  );
}
