import {
  LOCAL_ASR_KEEP_LOADED_OPTIONS,
  SHERPA_ONNX_ASR_MODELS,
  isLocalAsrModelSupportedOnOs,
} from './localAsr';
import { de } from '../i18n/de';
import { en } from '../i18n/en';
import { es } from '../i18n/es';
import { fr } from '../i18n/fr';
import { ja } from '../i18n/ja';
import { ko } from '../i18n/ko';
import { zhCN } from '../i18n/zh-CN';
import { zhTW } from '../i18n/zh-TW';

function assertEqual(actual: boolean, expected: boolean, name: string) {
  if (actual !== expected) {
    throw new Error(`${name}: expected ${expected}, got ${actual}`);
  }
}

const qwen = { runtime: 'generic', family: 'qwen3' } as const;
const whisper = { runtime: 'generic', family: 'whisper' } as const;
const foundry = { runtime: 'foundry', family: 'whisper' } as const;
const sherpa = { runtime: 'sherpa_onnx', family: 'qwen3_asr' } as const;
for (const os of ['mac', 'win', 'android'] as const) {
  assertEqual(isLocalAsrModelSupportedOnOs(qwen, os), os === 'mac', `generic Qwen on ${os}`);
  assertEqual(isLocalAsrModelSupportedOnOs(whisper, os), os === 'mac', `whisper.cpp on ${os}`);
  assertEqual(isLocalAsrModelSupportedOnOs(foundry, os), os === 'win', `Foundry Whisper on ${os}`);
  assertEqual(isLocalAsrModelSupportedOnOs(sherpa, os), os === 'win', `Sherpa Qwen on ${os}`);
}
// Model names/families overlap across runtimes. A Windows Qwen must never be
// treated as a generic macOS model merely because it has a qwen3-asr prefix.
assertEqual(isLocalAsrModelSupportedOnOs(sherpa, 'mac'), false, 'Windows Qwen is not macOS MLX');
assertEqual(
  isLocalAsrModelSupportedOnOs({ runtime: 'generic', family: 'unknown' }, 'mac'),
  false,
  'unknown models do not default to available',
);

const keepLoadedSeconds = LOCAL_ASR_KEEP_LOADED_OPTIONS.map((option) => option.seconds);
if (JSON.stringify(keepLoadedSeconds) !== JSON.stringify([0, 60, 300, 1800, 86400])) {
  throw new Error(`unexpected keep-loaded options: ${keepLoadedSeconds.join(', ')}`);
}

const fourierOption = SHERPA_ONNX_ASR_MODELS.find((m) => m.alias === 'oruk-fourier-2b-int8');
if (!fourierOption) {
  throw new Error('oruk-fourier-2b-int8 missing from SHERPA_ONNX_ASR_MODELS');
}
for (const [locale, bundle] of Object.entries({ en, zhCN, zhTW, ja, ko, de, es, fr })) {
  const label = bundle.localAsr.sherpaModelOrukFourier2B;
  const desc = bundle.localAsr.sherpaModelOrukFourier2BDesc;
  if (!label || !desc || !desc.includes('CC BY-NC 4.0')) {
    throw new Error(`missing or incomplete Oruk Fourier 2B i18n entry in ${locale}`);
  }
}
