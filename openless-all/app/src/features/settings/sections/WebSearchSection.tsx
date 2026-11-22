import { TextField } from '../../../ui/controls/TextField';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { readCredential, setCredential } from '../../../api/providers';
import { Button as Btn } from '../../../ui/controls/Button';
import { Card } from '../../../ui/layout/Card';
import { SelectLite } from '../../../ui/controls/SelectLite';
import { SettingRow } from '../../../ui/controls/SettingRow';
import { useInteraction } from '../../../ui/overlays/InteractionProvider';

function isInteractive(element: HTMLElement | null) {
  return element?.isConnected && !element.closest('[inert]');
}

export interface WebSearchConfig {
  provider: 'auto' | 'free' | 'tavily' | 'brave' | 'searxng' | 'off';
  apiKey: string;
  endpoint: string;
}
export const defaultSearchConfig: WebSearchConfig = {
  provider: 'free',
  apiKey: '',
  endpoint: '',
};
export async function loadSearchConfig(): Promise<WebSearchConfig> {
  const raw = await readCredential('web_search.config');
  const parsed = raw ? JSON.parse(raw) : {};
  return {
    provider: parsed.provider === 'auto' ? 'free' : (parsed.provider ?? 'free'),
    apiKey: parsed.apiKey ?? '',
    endpoint: parsed.endpoint ?? '',
  };
}

export function saveSearchConfig(value: WebSearchConfig): Promise<void> {
  return setCredential('web_search.config', JSON.stringify(value));
}
export function WebSearchSection() {
  const { t, i18n } = useTranslation();
  const text = (zh: string, en: string) => (i18n.language.startsWith('zh') ? zh : en);
  const heading = useRef<HTMLHeadingElement>(null);
  const { notify } = useInteraction();
  const [config, setConfig] = useState<WebSearchConfig | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let disposed = false;
    let observer: MutationObserver | undefined;
    void loadSearchConfig()
      .then((value) => {
        if (!disposed) setConfig(value);
      })
      .catch(() => {
        if (disposed) return;
        const announce = () => {
          if (!isInteractive(heading.current)) return false;
          notify({
            state: 'failed',
            message: i18n.language.startsWith('zh')
              ? '读取失败，请重新打开设置。'
              : 'Could not load settings. Please reopen.',
          });
          return true;
        };
        if (announce()) return;
        // An incoming form can finish loading before it becomes interactive.
        // A retained or exiting form must not publish feedback for the next page.
        const surface = heading.current?.closest<HTMLElement>('[data-transition-phase]');
        const entering = () =>
          surface?.dataset.transitionPhase === 'waiting' ||
          surface?.dataset.transitionPhase === 'enter';
        if (surface && entering()) {
          observer = new MutationObserver(() => {
            if (announce() || !entering()) observer?.disconnect();
          });
          observer.observe(surface, {
            attributes: true,
            attributeFilter: ['data-transition-phase'],
          });
        }
      });
    return () => {
      disposed = true;
      observer?.disconnect();
    };
  }, [i18n, notify]);
  return (
    <Card>
      <h3 ref={heading}>Web Search</h3>
      <p>
        {text(
          '为提问提供联网搜索。搜索服务会接收搜索词与 IP。',
          'Search the web for questions. Providers receive queries and your IP.',
        )}
      </p>
      {config && (
        <>
          <SettingRow label={text('搜索服务', 'Search provider')}>
            <SelectLite
              ariaLabel={text('搜索服务', 'Search provider')}
              value={config.provider}
              onChange={(provider) => setConfig({ ...config, provider: provider as WebSearchConfig['provider'], apiKey: '', endpoint: '' })}
              options={[
                { value: 'free', label: text('免费搜索', 'Free Search') },
                { value: 'tavily', label: 'Tavily' },
                { value: 'brave', label: 'Brave Search' },
                { value: 'searxng', label: 'SearXNG' },
                { value: 'off', label: text('关闭', 'Off') },
              ]}
            />
          </SettingRow>
          {['tavily', 'brave'].includes(config.provider) && (
            <SettingRow label="API Key">
              <TextField
                aria-label="Search API Key"
                type="password"
                autoComplete="off"
                style={{ maxWidth: 360 }}
                value={config.apiKey}
                onChange={(e) => setConfig({ ...config, apiKey: e.target.value })}
              />
            </SettingRow>
          )}
          {config.provider === 'searxng' && (
            <SettingRow
              label="SearXNG URL"
              desc={text(
                '填写已启用 JSON 输出的 /search 地址。',
                'Enter a /search endpoint with JSON output enabled.',
              )}
            >
              <TextField
                aria-label="SearXNG URL"
                type="url"
                placeholder="https://search.example.com/search"
                style={{ maxWidth: 360 }}
                value={config.endpoint}
                onChange={(e) => setConfig({ ...config, endpoint: e.target.value })}
              />
            </SettingRow>
          )}
          <Btn
            variant="primary"
            disabled={
              busy ||
              (['tavily', 'brave'].includes(config.provider) && !config.apiKey.trim()) ||
              (config.provider === 'searxng' && !/^https?:\/\//.test(config.endpoint))
            }
            onClick={async () => {
              setBusy(true);
              try {
                await saveSearchConfig(config);
                if (isInteractive(heading.current))
                  notify({ state: 'saved', message: t('common.saved') });
              } catch {
                if (isInteractive(heading.current)) {
                  notify({
                    state: 'failed',
                    message: text('保存失败，请重试。', 'Could not save. Please retry.'),
                  });
                }
              } finally {
                if (heading.current) setBusy(false);
              }
            }}
          >
            {text('保存', 'Save')}
          </Btn>
        </>
      )}
    </Card>
  );
}
