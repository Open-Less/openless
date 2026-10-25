import type { AudioDspConfig } from './types';

export const defaultAudioDsp: AudioDspConfig = {
  aecEnabled: false,
  outputBufferMs: 300,
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
