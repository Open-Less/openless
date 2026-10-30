import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { AudioDspConfig } from '../../lib/types';
import {
  getAndroidAudioEffectsStatus,
  setAndroidAudioMonitoring,
  type AndroidAudioEffectsStatus,
} from '../../lib/ipc/android-audio';
import { isTauri } from '../../lib/ipc/shared';
import { SettingRow, Toggle } from './shared';

type Spectrum = { raw: number[]; processed: number[] };

export function AndroidAudioEffects({
  value,
  onChange,
}: {
  value: AudioDspConfig;
  onChange: (patch: Partial<AudioDspConfig>) => void;
}) {
  const { t } = useTranslation();
  const label = (key: string, fallback: string) =>
    t(`settings.recording.dsp.${key}`, { defaultValue: fallback });
  const [status, setStatus] = useState<AndroidAudioEffectsStatus | null>(null);
  const [monitoring, setMonitoring] = useState(false);
  const [spectrum, setSpectrum] = useState<Spectrum | null>(null);
  const [error, setError] = useState('');
  useEffect(() => {
    let disposed = false;
    const refresh = () => {
      if (document.hidden) return;
      void getAndroidAudioEffectsStatus()
        .then((next) => {
          if (!disposed) setStatus(next);
        })
        .catch(() => {});
    };
    refresh();
    const timer = window.setInterval(refresh, 2000);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, []);
  useEffect(() => {
    if (!monitoring || !isTauri) {
      setSpectrum(null);
      return;
    }
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let timer: number | undefined;
    const renew = () => {
      if (document.hidden) setSpectrum(null);
      void setAndroidAudioMonitoring(!document.hidden).catch((reason: unknown) => {
        if (!disposed) setError(String(reason));
      });
    };
    void import('@tauri-apps/api/event')
      .then(async ({ listen }) => {
        const stop = await listen<Spectrum>('android-audio-spectrum', ({ payload }) => {
          if (disposed || document.hidden) return;
          if (payload.raw.length === 64 && payload.processed.length === 64) setSpectrum(payload);
        });
        if (disposed) {
          stop();
          return;
        }
        unlisten = stop;
        renew();
        timer = window.setInterval(renew, 1000);
      })
      .catch((reason: unknown) => {
        if (!disposed) setError(String(reason));
      });
    document.addEventListener('visibilitychange', renew);
    return () => {
      disposed = true;
      unlisten?.();
      window.clearInterval(timer);
      document.removeEventListener('visibilitychange', renew);
      void setAndroidAudioMonitoring(false).catch(() => {});
    };
  }, [monitoring]);
  const toggle = (
    field: 'androidAecEnabled' | 'androidNoiseSuppressionEnabled' | 'androidAgcEnabled',
    title: string,
    available: boolean,
  ) => (
    <div style={{ opacity: available ? 1 : 0.6 }}>
      <SettingRow
        label={title}
        desc={available ? undefined : label('unavailable', 'Unavailable on this device')}
      >
        <Toggle
          on={!!value[field]}
          onToggle={() => {
            if (available || value[field]) onChange({ [field]: !value[field] });
          }}
        />
      </SettingRow>
    </div>
  );
  return (
    <div style={{ marginTop: 12 }}>
      <p style={{ fontSize: 12, color: 'var(--ol-muted)' }}>
        {label(
          'androidEffectsHint',
          'Android system effects use the microphone session. They apply before the software chain; avoid enabling the same effect twice. Unsupported startup falls back to normal capture.',
        )}
      </p>
      {toggle(
        'androidAecEnabled',
        label('aec', 'System echo cancellation'),
        !!status?.aecAvailable,
      )}
      {toggle(
        'androidNoiseSuppressionEnabled',
        label('systemNoise', 'System noise suppression'),
        !!status?.noiseSuppressionAvailable,
      )}
      {toggle(
        'androidAgcEnabled',
        label('systemAgc', 'System automatic gain'),
        !!status?.agcAvailable,
      )}
      {status?.runtime?.error && (
        <p role="status" style={{ fontSize: 12 }}>
          {status.runtime.error}
        </p>
      )}
      {status?.runtime?.active && (
        <p style={{ fontSize: 12 }}>
          {label('effectsActive', 'System effects are active for this recording.')}
        </p>
      )}
      <SettingRow
        label={label('spectrum', 'Input / processed spectrum')}
        desc={label(
          'spectrumHint',
          'Shows the current enhanced recording only. This does not start a microphone, save audio, or upload it. Monitoring stops when this panel closes or goes to the background.',
        )}
      >
        <Toggle
          on={monitoring}
          onToggle={() => {
            setError('');
            setMonitoring(!monitoring);
          }}
        />
      </SettingRow>
      {monitoring && (
        <>
          <svg
            role="img"
            aria-label={label('spectrum', 'Input / processed spectrum')}
            viewBox="0 0 256 80"
            style={{ width: '100%', height: 100, background: 'var(--ol-bg)', borderRadius: 8 }}
          >
            {Array.from({ length: 64 }, (_, i) => {
              const raw = Math.max(0, Math.min(1, spectrum?.raw[i] || 0)) * 76;
              const processed = Math.max(0, Math.min(1, spectrum?.processed[i] || 0)) * 76;
              return (
                <g key={i}>
                  <rect
                    x={i * 4}
                    y={80 - raw}
                    width="3"
                    height={raw}
                    fill="currentColor"
                    opacity="0.25"
                  />
                  <rect
                    x={i * 4}
                    y={80 - processed}
                    width="2"
                    height={processed}
                    fill="var(--ol-accent, #6366f1)"
                  />
                </g>
              );
            })}
          </svg>
          <small>{label('spectrumLegend', 'Gray: input · Color: processed · 20 Hz–24 kHz')}</small>
          {!spectrum && (
            <p style={{ fontSize: 12 }}>
              {label(
                'spectrumWaiting',
                'Start a recording with speech enhancement enabled to see the spectrum.',
              )}
            </p>
          )}
        </>
      )}
      {error && <p role="status">{error}</p>}
    </div>
  );
}
