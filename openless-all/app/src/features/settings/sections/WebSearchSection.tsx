import { TextField } from '../../../ui/controls/TextField';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { readCredential, setCredential } from '../../../api/providers';
import { Button as Btn } from '../../../ui/controls/Button';
import { Card } from '../../../ui/layout/Card';
import { inputStyle } from '../../../ui/controls/settingsFieldStyles';
import { SettingRow } from '../../../ui/controls/SettingRow';
import { useInteraction } from '../../../ui/overlays/InteractionProvider';

function isInteractive(element: HTMLElement | null) {
  return element?.isConnected && !element.closest('[inert]');
}

export interface WebSearchConfig {
  provider:
    'auto' | 'free' | 'hybrid' | 'free_serp' | 'bing' | 'tavily' | 'brave' | 'searxng' | 'off';
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
          '划词追问和 FN + 空格随便问可由模型自主搜索。仅在需要时发送搜索词；搜索结果会作为参考资料交给当前模型。',
          'Selection Q&A and voice Ask can search when the model needs external information. Search queries go to the search provider; results go to your current model.',
        )}
      </p>
      <p>
        {text(
          '「免费搜索」默认从此设备查询 FreeSerp 和 Keenable，并合并结果，无需注册、API Key 或部署。服务会看到搜索词和请求 IP，并可能限流。「混合搜索」还会查询 DeepSeek，需要你自己的 API Key，可能产生费用。',
          'Free Search queries FreeSerp and Keenable from this device and merges the results. No account, API key, or setup is required. Providers can see the query and request IP and may rate-limit it. Hybrid Search also queries DeepSeek using your API key and may incur charges.',
        )}
      </p>
      <p>
        {text(
          'DeepSeek 大统一的旧问答仍使用其独立的原生搜索；这里的混合搜索不要求使用 DeepSeek 聊天模型。',
          'Legacy Q&A in DeepSeek Suite keeps its separate native search. Hybrid Search does not require a DeepSeek chat model.',
        )}
      </p>
      {config && (
        <>
          <SettingRow label={text('搜索服务', 'Search provider')}>
            <select
              aria-label="Web Search"
              style={inputStyle}
              value={config.provider}
              onChange={(e) =>
                setConfig({
                  ...config,
                  provider: e.target.value as WebSearchConfig['provider'],
                  apiKey: '',
                  endpoint: '',
                })
              }
            >
              <option value="free">{text('免费搜索', 'Free Search')}</option>
              <option value="hybrid">
                {text('混合搜索（免费 + DeepSeek）', 'Hybrid Search (Free + DeepSeek)')}
              </option>
              <option value="free_serp">FreeSerp</option>
              <option value="bing">Bing (RSS)</option>
              <option value="tavily">Tavily</option>
              <option value="brave">Brave Search</option>
              <option value="searxng">SearXNG</option>
              <option value="off">{text('关闭', 'Off')}</option>
            </select>
          </SettingRow>
          {['tavily', 'brave', 'hybrid'].includes(config.provider) && (
            <SettingRow label={config.provider === 'hybrid' ? 'DeepSeek API Key' : 'API Key'}>
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
                '可填写允许自动查询且已启用 JSON 的公开 /search 地址，不要求自建。',
                'You can use a public /search endpoint that permits automated queries and enables JSON. Self-hosting is not required.',
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
              (['tavily', 'brave', 'hybrid'].includes(config.provider) && !config.apiKey.trim()) ||
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
