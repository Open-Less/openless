import { MicIcon, PencilLineIcon, QuoteIcon, SquareIcon } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { chatPanelFocusKeyboard } from '../../api/qa';
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
  const voiceLabel = recording
    ? t('qa.voiceInputPlaceholder')
    : micBusy
      ? t('common.loading')
      : t('overview.inAppDictation.processing');
  const inputLabel = t('qa.composerPlaceholder');
  const editor = useQaDraftDictation(available && !embedded, onChange);

  return (
    <QaPromptBox
      value={value}
      label={inputLabel}
      placeholder={voiceActive ? voiceLabel : inputLabel}
      disabled={!available}
      readOnly={voiceActive}
      recording={recording}
      sendDisabled={busy}
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
            className={`qa-mic${recording ? ' is-recording' : ''}`}
            type="button"
            disabled={!available || generating || micBusy || (busy && !recording)}
            onClick={onToggleRecording}
            aria-pressed={recording}
            aria-label={recording ? t('qa.micStop') : t('qa.micLabel')}
            title={recording ? t('qa.micStop') : t('qa.micLabel')}
          >
            {recording ? <SquareIcon /> : <MicIcon />}
          </button>
        </>
      }
      feedback={
        voiceActive ? (
          <div className="qa-voice-feedback">
            <QaVoiceStrip level={level} recording={recording} label={voiceLabel} />
            {selectionPreview && (
              <span className="qa-recording-selection" title={selectionPreview}>
                <QuoteIcon />
                <span>{selectionPreview}</span>
              </span>
            )}
          </div>
        ) : !available ? (
          <span className="qa-browser-note">{t('qa.compact.browserUnavailable')}</span>
        ) : undefined
      }
    />
  );
}
