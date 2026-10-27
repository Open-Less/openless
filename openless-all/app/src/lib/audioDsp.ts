import type { AudioDspConfig } from './types';

export const defaultAudioDsp: AudioDspConfig = {
  aecMode: 'off',
  androidAecEnabled: false,
  androidNoiseSuppressionEnabled: false,
  androidAgcEnabled: false,
  noiseReduction: 'rnnoise',
  noiseStrength: 80,
  gainDb: 0,
  agcEnabled: true,
  agcTarget: 35,
  agcAttack: 50,
  agcDecay: 50,
  vadEnabled: false,
  vadThresholdDb: -50,
  dereverbLevel: 0,
  equalizerEnabled: false,
  equalizerPreampDb: 0,
  equalizerBandsDb: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  processingOrder: [
    'acousticEchoCancellation',
    'noiseReduction',
    'dereverb',
    'equalizer',
    'amplifier',
    'agc',
    'vad',
  ],
};

// MicYou EqualizerPanel.vue, pinned upstream revision in micyou-upstream.json.
export const audioEqualizerPresets = {
  normal: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  brightVocal: [-2, -1, 0, 1, 2, 3, 4, 4, 3, 2],
  deepVoice: [3, 4, 4, 3, 1, 0, -1, -2, -2, -1],
  podcast: [2, 3, 2, 0, -1, -1, 1, 2, 3, 2],
  vocalClarity: [-1, -1, 0, 1, 2, 3, 4, 3, 2, 1],
  warmVocal: [1, 2, 3, 2, 1, 0, -1, -1, 0, 1],
};

export const equalizerFrequencies = [
  '31 Hz',
  '63 Hz',
  '125 Hz',
  '250 Hz',
  '500 Hz',
  '1 kHz',
  '2 kHz',
  '4 kHz',
  '8 kHz',
  '16 kHz',
];
