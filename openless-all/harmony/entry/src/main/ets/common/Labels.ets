/** Human labels for provider ids, modes and channels shared by the native pages. */

interface LabelPair {
  id: string;
  label: string;
}

const PROVIDERS: LabelPair[] = [
  { id: 'volcengine', label: '火山引擎' },
  { id: 'elevenlabs', label: 'ElevenLabs' },
  { id: 'bailian', label: '阿里百炼' },
  { id: 'bailian-qwen3-realtime', label: '百炼 Qwen3 实时' },
  { id: 'bailian-fun-asr-flash', label: '百炼 Fun-ASR Flash' },
  { id: 'siliconflow', label: '硅基流动' },
  { id: 'stepfun', label: '阶跃星辰' },
  { id: 'stepfun-realtime', label: '阶跃星辰实时' },
  { id: 'zhipu', label: '智谱 GLM' },
  { id: 'minimax', label: 'MiniMax' },
  { id: 'groq', label: 'Groq' },
  { id: 'whisper', label: 'OpenAI Whisper' },
  { id: 'openai-whisper', label: 'OpenAI Whisper' },
  { id: 'openrouter', label: 'OpenRouter' },
  { id: 'openrouterFree', label: 'OpenRouter 免费' },
  { id: 'orcarouter', label: 'OrcaRouter' },
  { id: 'zenmux', label: 'ZenMux' },
  { id: 'openai-compatible', label: 'OpenAI 兼容接口' },
  { id: 'xiaomi-mimo-asr', label: '小米 MiMo 语音' },
  { id: 'iflytek', label: '讯飞 RTASR' },
  { id: 'xfyun', label: '讯飞 RTASR' },
  { id: 'tencent-cloud', label: '腾讯云' },
  { id: 'tencent', label: '腾讯云' },
  { id: 'ark', label: '火山方舟' },
  { id: 'deepseek', label: 'DeepSeek' },
  { id: 'openai', label: 'OpenAI' },
  { id: 'gemini', label: 'Gemini' },
  { id: 'atlascloud', label: 'Atlas Cloud' },
  { id: 'mimo', label: '小米 MiMo' },
  { id: 'cometapi', label: 'CometAPI' },
  { id: 'requesty', label: 'Requesty' },
  { id: 'api-route', label: 'API Route' },
  { id: 'alibabaCoding', label: '阿里云百炼 Coding' },
  { id: 'codingPlanX', label: 'Coding Plan X' },
  { id: 'opencode', label: 'OpenCode' },
  { id: 'tencentTokenHub', label: '腾讯混元 TokenHub' },
  { id: 'lmstudio', label: 'LM Studio' },
  { id: 'dashscope-omni', label: '阿里百炼全模态' },
  { id: 'omni-volcengine', label: '火山全模态' },
  { id: 'omni-gemini', label: 'Gemini 全模态' },
  { id: 'custom', label: '自定义接口' },
  { id: 'custom_responses', label: '自定义 Responses' },
  { id: 'custom_messages', label: '自定义 Messages' },
  { id: 'apple-speech', label: 'Apple 语音' },
  { id: 'local-qwen3', label: '本地 Qwen3' },
  { id: 'micyou', label: 'MicYou' },
  { id: 'raw', label: '原文直出' }
];

const PIPELINES: LabelPair[] = [
  { id: 'traditional', label: '两段式（转写 + 润色）' },
  { id: 'multimodal', label: '全模态（一次调用）' },
  { id: 'deepseek', label: 'DeepSeek 大统一' }
];

const POLISH_MODES: LabelPair[] = [
  { id: 'raw', label: '原文' },
  { id: 'light', label: '轻度润色' },
  { id: 'structured', label: '清晰结构' },
  { id: 'formal', label: '正式表达' }
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
