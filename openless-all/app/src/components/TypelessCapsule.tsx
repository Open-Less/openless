import { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { cancelDictation, stopDictation } from '../lib/ipc/dictation';
import type { CapsuleState, CapsuleStyle } from '../lib/types';
import { Icon } from './Icon';
import { VoiceOrbStage } from './VoiceOrbStage';
import './CapsuleStyles.css';

interface TypelessCapsuleProps {
  state: CapsuleState;
  level: number;
  message?: string;
  insertedChars?: number;
  operating?: boolean;
  translation?: boolean;
  warming?: boolean;
  preview?: boolean;
}

const WAVE_ENVELOPE = [0.28, 0.44, 0.63, 0.82, 0.96, 1, 0.96, 0.82, 0.63, 0.44, 0.28];

export function CapsuleWaveform({ level, warming = false }: { level: number; warming?: boolean }) {
  const voice = Math.min(1, Math.max(0, (level - 0.012) / 0.328));
  const amplitude = warming ? 0 : Math.pow(voice, 0.55);
  return (
    <span className="ol-capsule-waveform" data-warming={warming} aria-hidden="true">
      {WAVE_ENVELOPE.map((envelope, index) => (
        <span key={index} style={{ transform: `scaleY(${0.12 + amplitude * envelope * 0.88})` }} />
      ))}
    </span>
  );
}

/** A single shell changes width between recording and processing, so its outline stays stable. */
export function TypelessCapsule({
  state,
  level,
  message,
  insertedChars = 0,
  operating = false,
  translation = false,
  warming = false,
  preview = false,
}: TypelessCapsuleProps) {
  const { t } = useTranslation();
  const recording = state === 'recording';
  const processing = state === 'transcribing' || state === 'polishing';
  const label = processing
    ? t(operating ? 'capsule.using' : 'capsule.thinking')
    : state === 'done'
      ? message || t('capsule.inserted', { count: insertedChars })
      : state === 'cancelled'
        ? t('capsule.cancelled')
        : message || t('capsule.error');
  const cancel = useCallback(() => void cancelDictation(), []);
  const confirm = useCallback(() => void stopDictation(), []);

  return (
    <div className="ol-typeless-capsule-wrap" data-preview={preview}>
      {translation && <span className="ol-typeless-translation">{t('capsule.translating')}</span>}
      <div
        className="ol-typeless-capsule"
        data-recording={recording}
        data-error={state === 'error'}
        data-processing={processing}
      >
        <div className="ol-typeless-recording" aria-hidden={!recording}>
          <button
            className="ol-typeless-action"
            aria-label={t('common.cancel')}
            disabled={!recording || preview}
            onMouseDown={(event) => event.preventDefault()}
            onClick={cancel}
          >
            <Icon name="close" size={24} strokeWidth={2} />
          </button>
          <CapsuleWaveform level={level} warming={warming} />
          <button
            className="ol-typeless-action ol-typeless-confirm"
            aria-label={t('settings.shortcuts.confirm')}
            disabled={!recording || preview}
            onMouseDown={(event) => event.preventDefault()}
            onClick={confirm}
          >
            <Icon name="check" size={25} strokeWidth={2.1} />
          </button>
        </div>
        <div className="ol-typeless-status" aria-hidden={recording}>
          <span role="status" title={label}>
            {label}
          </span>
          {processing && !preview && (
            <button
              className="ol-typeless-stop"
              aria-label={t('common.cancel')}
              onMouseDown={(event) => event.preventDefault()}
              onClick={cancel}
            >
              <Icon name="close" size={13} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

/** This local preview makes a saved choice visible without starting the microphone. */
export function CapsuleStylePreview({ style }: { style: CapsuleStyle }) {
  return (
    <div className="ol-capsule-style-preview" data-style={style} aria-hidden="true">
      {style === 'typeless' ? (
        <TypelessCapsule state="recording" level={0.2} preview />
      ) : style === 'classic' ? (
        <div className="ol-classic-capsule-preview">
          <span>
            <Icon name="close" size={13} />
          </span>
          <CapsuleWaveform level={0.2} />
          <span>
            <Icon name="check" size={13} />
          </span>
        </div>
      ) : (
        <div className="ol-siri-capsule-preview">
          <VoiceOrbStage os="mac" state="recording" level={0.2} />
        </div>
      )}
    </div>
  );
}
