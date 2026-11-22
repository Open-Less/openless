import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  ensureDeepSeekSuiteConfig,
  saveDeepSeekSuiteConfig,
  type DeepSeekSuiteConfig,
} from './deepseekSuite';
import { useInteraction } from '../../ui/overlays/InteractionProvider';
import { Button as Btn } from '../../ui/controls/Button';
import { Card } from '../../ui/layout/Card';
import { inputStyle } from '../../ui/controls/settingsFieldStyles';
import { SettingRow } from '../../ui/controls/SettingRow';

export function DeepSeekSuiteSection() {
  const { t, i18n } = useTranslation();
  const text = (zh: string, en: string) => (i18n.language.startsWith('zh') ? zh : en);
  const [config, setConfig] = useState<DeepSeekSuiteConfig | null>(null);
  const [busy, setBusy] = useState(false);
  const saving = useRef(false);
  const alive = useRef(false);
  const { notify } = useInteraction('deepseek-suite');
  useEffect(() => {
    let active = true;
    alive.current = true;
    void ensureDeepSeekSuiteConfig()
      .then((value) => {
        if (active) setConfig(value);
      })
      .catch(() => {
        if (active)
          notify({
            state: 'failed',
            message: text('读取配置失败，请重试。', 'Could not load configuration.'),
          });
      });
    return () => {
      active = false;
      alive.current = false;
    };
  }, []);
  return (
    <Card>
      <h3>{text('DeepSeek 大统一', 'DeepSeek suite')}</h3>
      <p>
        {text(
          '当前管线使用 DeepSeek 完成语音识别、对话、原生联网搜索和朗读。此处凭据独立保存，首次使用会复制已配置的默认 DeepSeek 官方渠道 API Key。',
          'This pipeline uses DeepSeek for speech recognition, conversation, native web search and read aloud. Credentials are stored independently; first setup copies the default configured official DeepSeek channel key.',
        )}
      </p>
      <p>
        {text(
          'ASR 在松开录音键后上传，每段最长 120 秒。联网和语音按 DeepSeek 官方计费。传统、多模态管线仍使用各自的模型和 Web Search 配置。',
          'ASR uploads after recording stops, in segments of up to 120 seconds. DeepSeek charges apply to search and speech. Traditional and multimodal pipelines keep their own models and Web Search settings.',
        )}
      </p>
      {config && (
        <>
          <SettingRow label="DeepSeek API Key">
            <input
              aria-label="DeepSeek suite API Key"
              type="password"
              autoComplete="off"
              style={inputStyle}
              value={config.apiKey}
              disabled={busy}
              onChange={(e) => setConfig({ ...config, apiKey: e.target.value })}
            />
          </SettingRow>
          <SettingRow label={text('朗读音色', 'Voice')}>
            <select
              aria-label={text('朗读音色', 'Voice')}
              style={inputStyle}
              value={config.voice}
              disabled={busy}
              onChange={(e) =>
                setConfig({ ...config, voice: e.target.value as DeepSeekSuiteConfig['voice'] })
              }
            >
              <option value="stella">{text('海星 · 女声', 'Stella')}</option>
              <option value="tide">{text('暗潮 · 男声', 'Tide')}</option>
            </select>
          </SettingRow>
          <Btn
            variant="primary"
            disabled={busy}
            onClick={async () => {
              if (saving.current) return;
              saving.current = true;
              setBusy(true);
              const notice = notify({
                key: 'deepseek-suite-save',
                state: 'saving',
                message: t('common.saving'),
              });
              try {
                await saveDeepSeekSuiteConfig({ ...config, apiKey: config.apiKey.trim() });
                notice.update({
                  state: 'saved',
                  message: text('DeepSeek 配置已保存', 'DeepSeek configuration saved'),
                });
              } catch {
                notice.update({ state: 'failed', message: t('common.operationFailed') });
              } finally {
                saving.current = false;
                if (alive.current) setBusy(false);
              }
            }}
          >
            {text('保存配置', 'Save configuration')}
          </Btn>
        </>
      )}
    </Card>
  );
}
