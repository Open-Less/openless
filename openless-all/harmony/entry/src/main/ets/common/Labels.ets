/** Human labels for provider ids, modes and channels shared by the native pages. */

interface LabelPair {
  id: string;
  label: string;
}

const PROVIDERS: LabelPair[] = [
  { id: 'volcengine', label: '火山引擎' },
  { id: 'apple-speech', label: 'Apple 语音' },
  { id: 'local-qwen3', label: '本地 Qwen3' },
  { id: 'openai-whisper', label: 'OpenAI Whisper' },
  { id: 'openai', label: 'OpenAI' },
  { id: 'xfyun', label: '讯飞' },
  { id: 'tencent', label: '腾讯云' },
  { id: 'siliconflow', label: '硅基流动' },
  { id: 'ark', label: '火山方舟' },
  { id: 'deepseek', label: 'DeepSeek' },
  { id: 'bailian', label: '阿里百炼' },
  { id: 'gemini', label: 'Gemini' },
  { id: 'micyou', label: 'MicYou' },
  { id: 'omni-volcengine', label: '火山全模态' },
  { id: 'omni-gemini', label: 'Gemini 全模态' },
  { id: 'raw', label: '原文直出' }
];

const PIPELINES: LabelPair[] = [
  { id: 'traditional', label: '两段式（转写 + 润色）' },
  { id: 'multimodal', label: '全模态（一次调用）' }
];

const POLISH_MODES: LabelPair[] = [
  { id: 'raw', label: '原文' },
  { id: 'polish', label: '润色' },
  { id: 'translate', label: '翻译' },
  { id: 'structured', label: '结构化' }
];

const CHANNELS: LabelPair[] = [
  { id: 'official', label: '正式版' },
  { id: 'beta', label: 'Beta' }
];

function lookup(list: LabelPair[], id: string): string {
  for (let index = 0; index < list.length; index++) {
    if (list[index].id === id) return list[index].label;
  }
  return '';
}

export class Labels {
  static provider(id: string | undefined): string {
    if (id === undefined || id.length === 0) return '未配置';
    const known: string = lookup(PROVIDERS, id);
    return known.length > 0 ? known : id;
  }

  static pipeline(id: string | undefined): string {
    if (id === undefined || id.length === 0) return '默认';
    const known: string = lookup(PIPELINES, id);
    return known.length > 0 ? known : id;
  }

  static polishMode(id: string | undefined): string {
    if (id === undefined || id.length === 0) return '润色';
    const known: string = lookup(POLISH_MODES, id);
    return known.length > 0 ? known : id;
  }

  static channel(id: string | undefined): string {
    if (id === undefined || id.length === 0) return '正式版';
    const known: string = lookup(CHANNELS, id);
    return known.length > 0 ? known : id;
  }
}
