import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { AudioDspConfig, AudioDspStage } from '../../lib/types';
import { defaultAudioDsp, equalizerFrequencies } from '../../lib/audioDsp';
import { SelectLite } from '../../components/ui/SelectLite';
import { SettingRow, Toggle } from './shared';

export function AudioDspSettings({
  value,
  onChange,
}: {
  value?: AudioDspConfig;
  onChange: (value: AudioDspConfig) => void;
}) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<AudioDspConfig>({ ...defaultAudioDsp, ...value });
  useEffect(() => setDraft({ ...defaultAudioDsp, ...value }), [value]);
  const label = (key: string, fallback: string) =>
    t(`settings.recording.dsp.${key}`, { defaultValue: fallback });
  const save = (patch: Partial<AudioDspConfig>) => {
    const next = { ...draft, ...patch };
    setDraft(next);
    onChange(next);
  };
  const slider = (
    field: keyof AudioDspConfig,
    title: string,
    min: number,
    max: number,
    suffix = '',
  ) => (
    <SettingRow key={field} label={title}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, width: 'min(280px, 100%)' }}>
        <input
          aria-label={title}
          type="range"
          min={min}
          max={max}
          step={1}
          style={{ flex: 1, minWidth: 60 }}
          value={draft[field] as number}
          onChange={(event) =>
            setDraft((previous) => ({ ...previous, [field]: Number(event.target.value) }))
          }
          onPointerUp={() => onChange(draft)}
          onKeyUp={() => onChange(draft)}
        />
        <output style={{ minWidth: 52, textAlign: 'right', fontVariantNumeric: 'tabular-nums' }}>
          {draft[field] as number}
          {suffix}
        </output>
      </div>
    </SettingRow>
  );
  const stageLabels: Record<AudioDspStage, string> = {
    noiseReduction: label('noise', 'Noise reduction'),
    dereverb: label('dereverb', 'Comb filter'),
    equalizer: label('equalizer', 'Equalizer'),
    amplifier: label('gain', 'Input gain'),
    agc: label('agc', 'Automatic gain'),
    vad: label('vad', 'Speech gate'),
  };
  const moveStage = (index: number, direction: number) => {
    const order = [...draft.processingOrder];
    const target = index + direction;
    if (target < 0 || target >= order.length) return;
    [order[index], order[target]] = [order[target], order[index]];
    save({ processingOrder: order });
  };
  return (
    <details style={{ margin: '8px 0 16px' }}>
      <summary style={{ cursor: 'pointer', fontSize: 13 }}>
        {label('title', 'Speech enhancement settings')}
      </summary>
      <p style={{ fontSize: 12, color: 'var(--ol-muted)', lineHeight: 1.6 }}>
        {label(
          'description',
          'RNNoise is lightweight; PureVox offers model-based noise reduction with higher CPU use. Changes apply to the next recording.',
        )}
      </p>
      <SettingRow label={stageLabels.noiseReduction}>
        <SelectLite
          ariaLabel={stageLabels.noiseReduction}
          value={draft.noiseReduction}
          onChange={(v) => save({ noiseReduction: v as AudioDspConfig['noiseReduction'] })}
          options={[
            { value: 'off', label: label('off', 'Off') },
            { value: 'rnnoise', label: 'RNNoise' },
            { value: 'purevox', label: 'PureVox 6' },
            { value: 'spectral', label: label('spectral', 'Spectral subtraction') },
          ]}
        />
      </SettingRow>
      {draft.noiseReduction !== 'off' &&
        slider('noiseStrength', label('strength', 'Noise reduction strength'), 0, 100, '%')}
      {slider('gainDb', stageLabels.amplifier, -30, 30, ' dB')}
      <SettingRow label={stageLabels.agc}>
        <Toggle on={draft.agcEnabled} onToggle={() => save({ agcEnabled: !draft.agcEnabled })} />
      </SettingRow>
      {draft.agcEnabled && (
        <>
          {slider('agcTarget', label('target', 'Target level'), 5, 90, '%')}
          {slider('agcAttack', label('attack', 'Attack speed'), 1, 100)}
          {slider('agcDecay', label('decay', 'Release speed'), 1, 100)}
        </>
      )}
      <SettingRow
        label={stageLabels.vad}
        desc={label(
          'vadHint',
          'Attenuates quiet passages; it does not stop recording. A high threshold can remove quiet speech.',
        )}
      >
        <Toggle on={draft.vadEnabled} onToggle={() => save({ vadEnabled: !draft.vadEnabled })} />
      </SettingRow>
      {draft.vadEnabled &&
        slider('vadThresholdDb', label('threshold', 'Speech gate threshold'), -80, -10, ' dB')}
      <details style={{ marginTop: 10 }}>
        <summary style={{ cursor: 'pointer', fontSize: 12 }}>
          {label('tone', 'Tone and room adjustments')}
        </summary>
        <p style={{ fontSize: 12, color: 'var(--ol-muted)' }}>
          {label(
            'combHint',
            'The comb filter follows MicYou’s simple delay subtraction. It can color speech; start at 0%.',
          )}
        </p>
        {slider('dereverbLevel', stageLabels.dereverb, 0, 100, '%')}
        <SettingRow label={stageLabels.equalizer}>
          <Toggle
            on={draft.equalizerEnabled}
            onToggle={() => save({ equalizerEnabled: !draft.equalizerEnabled })}
          />
        </SettingRow>
        {draft.equalizerEnabled && (
          <>
            {slider('equalizerPreampDb', label('preamp', 'Equalizer preamp'), -20, 20, ' dB')}
            <div
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(110px, 1fr))',
                gap: 12,
                padding: '12px 0',
              }}
            >
              {equalizerFrequencies.map((frequency, index) => (
                <label key={frequency} style={{ fontSize: 12 }}>
                  {frequency} · {draft.equalizerBandsDb[index]} dB
                  <input
                    aria-label={`${stageLabels.equalizer} ${frequency}`}
                    type="range"
                    min={-12}
                    max={12}
                    step={1}
                    style={{ width: '100%' }}
                    value={draft.equalizerBandsDb[index]}
                    onChange={(event) => {
                      const bands = [...draft.equalizerBandsDb];
                      bands[index] = Number(event.target.value);
                      setDraft((previous) => ({ ...previous, equalizerBandsDb: bands }));
                    }}
                    onPointerUp={() => onChange(draft)}
                    onKeyUp={() => onChange(draft)}
                  />
                </label>
              ))}
            </div>
          </>
        )}
      </details>
      <details style={{ marginTop: 10 }}>
        <summary style={{ cursor: 'pointer', fontSize: 12 }}>
          {label('order', 'Processing order')}
        </summary>
        <ol style={{ paddingLeft: 20 }}>
          {draft.processingOrder.map((stage, index) => (
            <li key={stage} style={{ fontSize: 12, marginTop: 8 }}>
              <span style={{ display: 'inline-block', minWidth: 140 }}>{stageLabels[stage]}</span>
              <button
                type="button"
                disabled={index === 0}
                aria-label={`${label('up', 'Move up')} ${stageLabels[stage]}`}
                onClick={() => moveStage(index, -1)}
              >
                ↑
              </button>{' '}
              <button
                type="button"
                disabled={index === draft.processingOrder.length - 1}
                aria-label={`${label('down', 'Move down')} ${stageLabels[stage]}`}
                onClick={() => moveStage(index, 1)}
              >
                ↓
              </button>
            </li>
          ))}
        </ol>
      </details>
      <button type="button" style={{ marginTop: 14 }} onClick={() => save(defaultAudioDsp)}>
        {label('reset', 'Reset enhancement settings')}
      </button>
    </details>
  );
}
