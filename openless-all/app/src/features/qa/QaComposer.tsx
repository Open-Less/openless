import { MicIcon, PencilLineIcon, QuoteIcon, SquareIcon } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { QaPromptBox } from './QaPromptBox';
import { VoiceWaveform } from './chat/VoiceWaveform';

export type QaStatus = 'idle' | 'recording' | 'thinking' | 'error';

interface QaComposerProps {
  value: string;
  status: QaStatus;
  level: number;
  voiceActive: boolean;
  selectionPreview: string;
  busy: boolean;
  micBusy: boolean;
  available: boolean;
  editInstructionMode: boolean;
  onEditInstructionModeChange: (enabled: boolean) => void;
  onChange: (value: string) => void;
  onSubmit: () => void;
  onToggleRecording: () => void;
  onRequestKeyboard: () => void;
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
  available,
  editInstructionMode,
  onEditInstructionModeChange,
  onChange,
  onSubmit,
  onToggleRecording,
  onRequestKeyboard,
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

  return (
    <QaPromptBox
      value={value}
      label={inputLabel}
      placeholder={voiceActive ? voiceLabel : inputLabel}
      disabled={!available}
      readOnly={voiceActive}
      sendDisabled={busy}
      sendLabel={t('qa.composerSend')}
      stop={
        generating
          ? { label: t('qa.stop'), disabled: !available || cancelBusy, onClick: onCancelTurn }
          : undefined
      }
      onChange={onChange}
      onSubmit={onSubmit}
      onRequestKeyboard={onRequestKeyboard}
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
            <VoiceWaveform
              level={recording ? level : 0}
              processing={!recording}
              label={voiceLabel}
            />
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
