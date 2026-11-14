import { isOfficialDeepSeekEndpoint } from '../../lib/deepseekEndpoint';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  createChannel,
  listChannels,
  readCredential,
  setCredential,
  setChannelEnabled,
  reorderChannels,
} from '../../lib/ipc';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Btn, Card } from '../_atoms';
import { inputStyle, SettingRow } from './shared';
import { loadSearchConfig, saveSearchConfig } from './WebSearchSection';

export function DeepSeekSuiteSection() {
  const { i18n } = useTranslation();
  const text = (zh: string, en: string) => (i18n.language.startsWith('zh') ? zh : en);
  const { updatePrefs } = useHotkeySettings();
  const [key, setKey] = useState('');
  const [voice, setVoice] = useState<'stella' | 'tide'>('stella');
  const [busy, setBusy] = useState(false);
  const [ready, setReady] = useState(false);
  const [status, setStatus] = useState('');
  useEffect(() => {
    void (async () => {
      const channels = await listChannels('llm');
      const configured = channels.find((c) => c.providerType === 'deepseek');
      if (configured) {
        const endpoint =
          (await readCredential('ark.endpoint', configured.id)) || 'https://api.deepseek.com/v1';
        if (isOfficialDeepSeekEndpoint(endpoint)) {
          setKey((await readCredential('ark.api_key', configured.id)) ?? '');
        }
      }
      const config = await loadSearchConfig();
      setVoice(config.voice);
      setReady(true);
    })().catch(() =>
      setStatus(text('读取失败，请重新打开设置。', 'Could not load settings. Please reopen.')),
    );
  }, []);
  return (
    <Card>
      <h3>{text('DeepSeek 大统一', 'DeepSeek suite')}</h3>
      <p>
        {text(
          '一个 API Key 配置语音识别、deepseek-flash 对话与原生联网、流式语音朗读。启用后使用传统 ASR + LLM 管线；回答旁的朗读按钮使用 DeepSeek。',
          'Use one API key for speech recognition, deepseek-flash with native web search, and streaming speech. Enabling selects the ASR + LLM pipeline. Read-aloud buttons use DeepSeek.',
        )}
      </p>
      <p>
        {text(
          'ASR 在松开录音键后上传，每段最长 120 秒。联网和语音按 DeepSeek 官方计费；可在高级功能 → Web Search 改用免费搜索或关闭。',
          'ASR uploads after recording stops, in segments of at most 120 seconds. Search and voice incur DeepSeek charges. Choose free search or disable search in Advanced → Web Search.',
        )}
      </p>
      <SettingRow label="DeepSeek API Key">
        <input
          aria-label="DeepSeek API Key"
          type="password"
          autoComplete="off"
          style={inputStyle}
          value={key}
          disabled={!ready || busy}
          onChange={(e) => setKey(e.target.value)}
        />
      </SettingRow>
      <SettingRow label={text('朗读音色', 'Voice')}>
        <select
          aria-label={text('朗读音色', 'Voice')}
          style={inputStyle}
          value={voice}
          onChange={(e) => setVoice(e.target.value as 'stella' | 'tide')}
        >
          <option value="stella">{text('海星 · 女声', 'Stella')}</option>
          <option value="tide">{text('暗潮 · 男声', 'Tide')}</option>
        </select>
      </SettingRow>
      <Btn
        variant="primary"
        disabled={!ready || busy || !key.trim()}
        onClick={async () => {
          setBusy(true);
          setStatus('');
          try {
            let llmId = '';
            const prepared: { kind: 'asr' | 'llm'; id: string }[] = [];
            for (const kind of ['asr', 'llm'] as const) {
              const channels = await listChannels(kind);
              const existing = channels.find((c) => c.providerType === 'deepseek');
              const id = existing?.id ?? (await createChannel(kind, 'deepseek', 'DeepSeek'));
              if (!existing) await setChannelEnabled(kind, id, false);
              const prefix = kind === 'asr' ? 'asr' : 'ark';
              await setCredential(`${prefix}.api_key`, key.trim(), id);
              await setCredential(`${prefix}.endpoint`, 'https://api.deepseek.com/v1', id);
              await setCredential(
                kind === 'asr' ? 'asr.model' : 'ark.model_id',
                kind === 'asr' ? 'deepseek-asr' : 'deepseek-flash',
                id,
              );
              if (kind === 'llm') {
                llmId = id;
                await setCredential('ark.request_format', 'chat_completions', id);
              }
              prepared.push({ kind, id });
            }
            await saveSearchConfig({ ...(await loadSearchConfig()), ttsChannel: llmId, voice });
            for (const { kind, id } of prepared) {
              await setChannelEnabled(kind, id, true);
              await reorderChannels(kind, [
                id,
                ...(await listChannels(kind)).filter((c) => c.id !== id).map((c) => c.id),
              ]);
            }
            await updatePrefs((p) => ({ ...p, pipelineMode: 'traditional' }));
            window.dispatchEvent(new Event('ol-channels-changed'));
            setStatus(
              text(
                '已启用。可在语言模型和语音识别中测试连接。',
                'Enabled. Test connections in Language models and Speech recognition.',
              ),
            );
          } catch {
            setStatus(
              text(
                '配置未全部完成，请重试。已保存的渠道可在模型服务中检查。',
                'Setup did not finish. Retry or inspect saved channels in Model services.',
              ),
            );
          } finally {
            setBusy(false);
          }
        }}
      >
        {busy
          ? text('配置中…', 'Configuring…')
          : text('配置并启用 DeepSeek', 'Configure and enable DeepSeek')}
      </Btn>
      {status && <p role="status">{status}</p>}
    </Card>
  );
}
