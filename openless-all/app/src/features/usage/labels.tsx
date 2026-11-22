import { useTranslation } from 'react-i18next';
import { formatUsageNumber } from '../../lib/serviceUsageMetrics';

export function useUsageLabels() {
  const { t, i18n } = useTranslation();
  const zh = i18n.resolvedLanguage?.startsWith('zh') ?? false;
  const text = (cn: string, en: string) => (zh ? cn : en);
  const serviceName = (service: string | null) => {
    if (!service) return text('未知服务', 'Unknown service');
    const labels: Record<string, string> = {
      anthropic: 'Claude · Anthropic',
      dashscope: text('阿里云百炼', 'Alibaba Cloud Bailian'),
      openrouter: 'OpenRouter',
      zhipu: text('智谱', 'Zhipu'),
      groq: 'Groq',
      zenmux: 'ZenMux',
      minimax: 'MiniMax',
    };
    return (
      labels[service] ??
      t(`settings.providers.presets.${service === 'api-route' ? 'apiRoute' : service}`, {
        defaultValue: service,
      })
    );
  };
  const protocolName = (protocol: string | null) =>
    ({ chat_completions: 'Chat Completions', responses: 'Responses', messages: 'Messages' })[
      protocol ?? ''
    ] ??
    protocol ??
    text('未知协议', 'Unknown protocol');
  const purposeName = (purpose: string) => {
    const labels: Record<string, [string, string]> = {
      dictationPostprocess: ['听写后处理', 'Dictation processing'],
      translation: ['翻译', 'Translation'],
      selection: ['选区处理', 'Selection'],
      question: ['问答', 'Questions'],
      quickNote: ['速记', 'Quick note'],
      voiceEdit: ['语音编辑', 'Voice editing'],
      cloudNote: ['云笔记', 'Cloud note'],
    };
    const label = labels[purpose];
    return label ? text(...label) : text('其他', 'Other');
  };
  const safeTokens = (value: number | null) => (
    <span
      title={
        value !== null && Number.isSafeInteger(value) && value >= 0
          ? value.toLocaleString()
          : undefined
      }
    >
      {formatUsageNumber(value)}
    </span>
  );

  return { text, serviceName, protocolName, purposeName, safeTokens };
}
