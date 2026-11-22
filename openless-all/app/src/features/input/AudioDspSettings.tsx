import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { AudioDspConfig, AudioDspStage } from '../../api/contracts';
import { audioEqualizerPresets, defaultAudioDsp, equalizerFrequencies } from '../../lib/audioDsp';
import { isAndroid } from '../../runtime/platform';
import { AndroidAudioEffects } from './AndroidAudioEffects';
import { SelectLite } from '../../components/ui/SelectLite';
import { SettingRow, Toggle } from '../../ui/controls/SettingRow';
import { SolidSlider } from '../../ui/controls/Slider';

export function AudioDspSettings({
  value,
  onChange,
  page = false,
}: {
  value?: AudioDspConfig;
  onChange: (value: AudioDspConfig) => void;
  /** The third settings screen already titles this panel, so skip the outer disclosure. */
  page?: boolean;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(page);
  const [draft, setDraft] = useState<AudioDspConfig>({ ...defaultAudioDsp, ...value });
  const draftRef = useRef(draft);
  draftRef.current = draft;
  useEffect(() => {
    const next = { ...defaultAudioDsp, ...value };
    draftRef.current = next;
    setDraft(next);
  }, [value]);
  const label = (key: string, fallback: string) =>
    t(`settings.recording.dsp.${key}`, { defaultValue: fallback });
  const save = (patch: Partial<AudioDspConfig>) => {
    const next = { ...draftRef.current, ...patch };
    draftRef.current = next;
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
        <SolidSlider
          aria-label={title}
          min={min}
          max={max}
          step={1}
          value={draft[field] as number}
          onValueChange={(value) => {
            const next = { ...draftRef.current, [field]: value };
            draftRef.current = next;
            setDraft(next);
          }}
          onValueCommitted={() => onChange(draftRef.current)}
        />
        <output style={{ minWidth: 52, textAlign: 'right', fontVariantNumeric: 'tabular-nums' }}>
          {draft[field] as number}
          {suffix}
        </output>
      </div>
    </SettingRow>
  );
  const stageLabels: Record<AudioDspStage, string> = {
    acousticEchoCancellation: label('aec', 'Echo cancellation'),
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
  const body = (
    <>
      <p style={{ fontSize: 12, color: 'var(--ol-muted)', lineHeight: 1.6 }}>
        {label(
          'description',
          'RNNoise is lightweight; PureVox offers model-based noise reduction with higher CPU use. Changes apply to the next recording.',
        )}
      </p>
      <SettingRow
        label={stageLabels.acousticEchoCancellation}
        desc={label(
          'aecHint',
          'Suppresses loudspeaker feedback while on speaker. It needs a speaker reference signal, so leave it off with headphones.',
        )}
      >
        <SelectLite
          ariaLabel={stageLabels.acousticEchoCancellation}
          value={draft.aecMode}
          onChange={(v) => save({ aecMode: v as AudioDspConfig['aecMode'] })}
          options={[
            { value: 'off', label: label('aecOff', 'Off') },
            { value: 'light', label: label('aecLight', 'Light (low CPU)') },
            { value: 'model', label: label('aecModel', 'Model (stronger)') },
          ]}
        />
      </SettingRow>
      {draft.aecMode === 'model' &&
        label(
          'aecModelHint',
          'The model cancels more echo but costs about 3x the CPU; prefer Light on older machines.',
        )}
      {isAndroid() && expanded && <AndroidAudioEffects value={draft} onChange={save} />}
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
      {slider('gainDb', stageLabels.amplifier, -50, 50, ' dB')}
      <SettingRow label={stageLabels.agc}>
        <Toggle on={draft.agcEnabled} onToggle={() => save({ agcEnabled: !draft.agcEnabled })} />
      </SettingRow>
      {draft.agcEnabled && (
        <>
          {slider('agcTarget', label('target', 'Target level'), 0, 100, '%')}
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
        slider('vadThresholdDb', label('threshold', 'Speech gate threshold'), -100, 0, ' dB')}
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
            <SettingRow label={label('presets', 'Equalizer presets')}>
              <SelectLite
                value=""
                ariaLabel={label('presets', 'Equalizer presets')}
                options={[
                  { value: '', label: label('choosePreset', 'Choose a preset') },
                  ...Object.keys(audioEqualizerPresets).map((key) => ({
                    value: key,
                    label: label(`preset_${key}`, key),
                  })),
                ]}
                onChange={(key) => {
                  const bands = audioEqualizerPresets[key as keyof typeof audioEqualizerPresets];
                  if (bands) save({ equalizerBandsDb: [...bands] });
                }}
              />
            </SettingRow>
            {slider('equalizerPreampDb', label('preamp', 'Equalizer preamp'), -20, 20, ' dB')}
            <div className="ol-grid-12 ol-grid-bands" style={{ padding: '12px 0' }}>
              {equalizerFrequencies.map((frequency, index) => (
                <label key={frequency} style={{ fontSize: 12 }}>
                  {frequency} · {draft.equalizerBandsDb[index]} dB
                  <SolidSlider
                    aria-label={`${stageLabels.equalizer} ${frequency}`}
                    min={-15}
                    max={15}
                    step={1}
                    style={{ width: '100%' }}
                    value={draft.equalizerBandsDb[index]}
                    onValueChange={(value) => {
                      const bands = [...draftRef.current.equalizerBandsDb];
                      bands[index] = value;
                      const next = { ...draftRef.current, equalizerBandsDb: bands };
                      draftRef.current = next;
                      setDraft(next);
                    }}
                    onValueCommitted={() => onChange(draftRef.current)}
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
      <button
        type="button"
        className="ol-pressable"
        style={{ marginTop: 14 }}
        onClick={() => save(defaultAudioDsp)}
      >
        {label('reset', 'Reset enhancement settings')}
      </button>
    </>
  );
  if (page) return body;
  return (
    <details
      style={{ margin: '8px 0 16px' }}
      onToggle={(event) => setExpanded(event.currentTarget.open)}
    >
      <summary style={{ cursor: 'pointer', fontSize: 13 }}>
        {label('title', 'Speech enhancement settings')}
      </summary>
      {body}
    </details>
  );
}
