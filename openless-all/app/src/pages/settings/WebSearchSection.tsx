import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { readCredential, setCredential } from '../../lib/ipc';
import { Btn, Card } from '../_atoms';
import { inputStyle, SettingRow } from './shared';

export interface WebSearchConfig {
  provider: 'auto' | 'free' | 'tavily' | 'brave' | 'searxng' | 'off';
  apiKey: string;
  endpoint: string;
  ttsChannel: string;
  voice: 'stella' | 'tide';
}
export const defaultSearchConfig: WebSearchConfig = {
  provider: 'auto',
  apiKey: '',
  endpoint: '',
  ttsChannel: '',
  voice: 'stella',
};
export async function loadSearchConfig(): Promise<WebSearchConfig> {
  const raw = await readCredential('web_search.config');
  return raw ? { ...defaultSearchConfig, ...JSON.parse(raw) } : { ...defaultSearchConfig };
}
export function saveSearchConfig(value: WebSearchConfig): Promise<void> {
  return setCredential('web_search.config', JSON.stringify(value));
}
export function WebSearchSection() {
  const { i18n } = useTranslation();
  const text = (zh: string, en: string) => (i18n.language.startsWith('zh') ? zh : en);
  const [config, setConfig] = useState<WebSearchConfig | null>(null);
  const [status, setStatus] = useState('');
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    void loadSearchConfig()
      .then(setConfig)
      .catch(() =>
        setStatus(text('读取失败，请重新打开设置。', 'Could not load settings. Please reopen.')),
      );
  }, []);
  return (
    <Card>
      <h3>Web Search</h3>
      <p>
        {text(
          '划词追问和 FN + 空格随便问可由模型自主搜索。仅在需要时发送搜索词；搜索结果会作为参考资料交给当前模型。',
          'Selection Q&A and voice Ask can search when the model needs external information. Search queries go to the search provider; results go to your current model.',
        )}
      </p>
      <p>
        {text(
          '自动：DeepSeek 官方 deepseek-flash 使用原生联网（按官方计费）；其他模型使用免费 Bing 网页搜索，无需 API Key。免费网页接口可能受网络或限流影响。',
          'Auto: official DeepSeek deepseek-flash uses native search (provider charges apply); other models use free Bing web search without an API key. Free search availability depends on connectivity and rate limits.',
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
              <option value="auto">{text('自动（推荐）', 'Auto (recommended)')}</option>
              <option value="free">{text('Bing 免费网页搜索', 'Bing free web search')}</option>
              <option value="tavily">Tavily</option>
              <option value="brave">Brave Search</option>
              <option value="searxng">SearXNG</option>
              <option value="off">{text('关闭', 'Off')}</option>
            </select>
          </SettingRow>
          {['tavily', 'brave'].includes(config.provider) && (
            <SettingRow label="API Key">
              <input
                aria-label="Search API Key"
                type="password"
                autoComplete="off"
                style={inputStyle}
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
              <input
                aria-label="SearXNG URL"
                type="url"
                placeholder="https://search.example.com/search"
                style={inputStyle}
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
              setStatus('');
              try {
                await saveSearchConfig(config);
                setStatus(text('已保存', 'Saved'));
              } catch {
                setStatus(text('保存失败，请重试。', 'Could not save. Please retry.'));
              } finally {
                setBusy(false);
              }
            }}
          >
            {text('保存', 'Save')}
          </Btn>
        </>
      )}
      {status && <p role="status">{status}</p>}
    </Card>
  );
}
