import { TextField } from '../../ui/controls/TextField';
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
import { SelectLite } from '../../ui/controls/SelectLite';
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
          '使用 DeepSeek 进行语音识别、对话、联网搜索和朗读。录音会发送至 DeepSeek，联网与语音按官方标准计费。',
          'Use DeepSeek for speech recognition, conversation, web search and read aloud. Recordings are sent to DeepSeek; search and speech are billed by DeepSeek.',
        )}
      </p>
      {config && (
        <>
          <SettingRow label="DeepSeek API Key">
            <TextField
              aria-label="DeepSeek suite API Key"
              type="password"
              autoComplete="off"
              style={{ maxWidth: 360 }}
              value={config.apiKey}
              disabled={busy}
              onChange={(e) => setConfig({ ...config, apiKey: e.target.value })}
            />
          </SettingRow>
          <SettingRow label={text('朗读音色', 'Voice')}>
            <SelectLite
              ariaLabel={text('朗读音色', 'Voice')}
              value={config.voice}
              disabled={busy}
              onChange={(voice) => setConfig({ ...config, voice: voice as DeepSeekSuiteConfig['voice'] })}
              options={[
                { value: 'stella', label: text('海星 · 女声', 'Stella') },
                { value: 'tide', label: text('暗潮 · 男声', 'Tide') },
              ]}
            />
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
