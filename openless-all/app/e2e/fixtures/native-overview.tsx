import { emit } from '@tauri-apps/api/event';
import { mockIPC } from '@tauri-apps/api/mocks';
import { createInstance } from 'i18next';
import { createRoot } from 'react-dom/client';
import { I18nextProvider } from 'react-i18next';
import { previewHistoryEntries } from '../../src/preview/fixtures';
import { getPreviewProfile } from '../../src/preview/profiles';
import { applyFontScale, type FontScaleId } from '../../src/lib/fontScale';
import { AppViewport } from '../../src/ui/layout/AppViewport';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import '../../src/styles/refinements.css';

const params = new URLSearchParams(location.search);
const dashboard = params.has('dashboard');
const state = params.get('state');
const profile = getPreviewProfile('mac');
const year = new Date().getFullYear();
const day = `${year}-01-01`;
let count = 2;
let reads = 0;
let deferred: (() => void) | undefined;
let activityDeferred: (() => void) | undefined;

mockIPC(
  (command, args) => {
    switch (command) {
      case 'get_startup_snapshot':
        return { backend: { running: true }, contractVersion: '2.0.0' };
      case 'get_settings_snapshot':
        return {
          revision: 1,
          preferences: { ...mockSettings, showOverviewActivityHeatmap: state !== 'disabled' },
        };
      case 'get_platform_capabilities':
        return profile.capabilities;
      case 'get_hotkey_capability':
        return profile.hotkeyCapability;
      case 'set_remote_locale':
        return;
      case 'get_credentials':
        if (state === 'service-error') throw new Error('Service status unavailable');
        return { ...mockCredentialsStatus, pipelineMode: 'deepseek' };
      case 'list_history':
        if (state === 'history-error') throw new Error('History unavailable');
        return Array.from({ length: 40 }, (_, index) => ({
          ...previewHistoryEntries[0],
          id: `dashboard-${index}`,
          finalText: `Record ${index + 1}: ${previewHistoryEntries[0].finalText}`,
        }));
      case 'get_activity_stats': {
        if (dashboard) {
          if (state === 'activity-error') throw new Error('Activity unavailable');
          return [{ date: day, count: 987654321 }];
        }
        const value = [{ date: day, count }];
        if (activityDeferred !== undefined) return value;
        if (count === 2)
          return new Promise((resolve) => {
            activityDeferred = () => resolve(value);
          });
        return value;
      }
      case 'get_activity_apps': {
        if ((args as { year: number }).year !== year) throw new Error('Expected current year');
        if (dashboard) {
          if (state === 'apps-error') throw new Error('App statistics unavailable');
          if (state === 'unsupported') throw new Error('get_activity_apps command not found');
          return Array.from({ length: 40 }, (_, index) => [`Application ${index + 1}`, 40 - index]);
        }
        reads++;
        const rows = [
          ['Browser', 1],
          ['Editor', count],
        ];
        if (reads !== 2) return rows;
        return new Promise((resolve) => {
          deferred = () => resolve(rows);
        });
      }
      default:
        throw new Error(`Unexpected command: ${command}`);
    }
  },
  { shouldMockEvents: true },
);

// Install the native bridge before imports that can initialize the shared history store.
const { mockSettings, mockCredentialsStatus } = await import('../../src/lib/ipc/mock-data');
const { useActivityStats } = await import('../../src/data/history');
const { ActivityHeatmap } = await import('../../src/features/overview/ActivityHeatmap');
const i18n = createInstance();
const { en } = await import('../../src/i18n/en');
const { de } = await import('../../src/i18n/de');
const { zhCN } = await import('../../src/i18n/zh-CN');
await i18n.init({
  lng: params.get('locale') ?? 'en',
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
  resources: {
    en: {
      translation: {
        ...en,
        overview: { ...en.overview, activityCount: '{{count}} dictations' },
      },
    },
    de: { translation: de },
    'zh-CN': { translation: zhCN },
  },
});

const publish = (revision: number) =>
  emit('backend:event', { kind: { type: 'history_changed', payload: { revision } } });

function Fixture() {
  const activity = useActivityStats();
  return (
    <main>
      <button onClick={() => activityDeferred?.()}>Complete initial activity read</button>
      <button onClick={() => void publish(1)}>Publish record write</button>
      <button
        onClick={() => {
          count = 43;
          void publish(2);
        }}
      >
        Publish activity write
      </button>
      <button onClick={() => deferred?.()}>Complete old app read</button>
      <button
        onClick={(event) => {
          event.currentTarget.textContent = String(reads);
        }}
      >
        App read count
      </button>
      {activity.data && <ActivityHeatmap activity={activity.data} />}
    </main>
  );
}
const { Overview } = await import('../../src/features/overview/Overview');
const { HotkeySettingsProvider } = await import('../../src/data/preferences');
const { InteractionProvider, TooltipProvider } = await import('../../src/ui/overlays');
applyFontScale((params.get('scale') as FontScaleId) ?? 'medium');
createRoot(document.getElementById('root')!).render(
  <I18nextProvider i18n={i18n}>
    <AppViewport scale={dashboard}>
      {dashboard ? (
        <HotkeySettingsProvider>
          <InteractionProvider>
            <TooltipProvider>
              <div style={{ display: 'flex', flex: 1, minWidth: 0, minHeight: 0 }}>
                <aside style={{ width: 226, flex: 'none' }} aria-label="Sidebar" />
                <main
                  data-ol-page="overview"
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    flex: 1,
                    minWidth: 0,
                    minHeight: 0,
                    padding: '38px 24px 24px',
                    overflow: 'hidden',
                  }}
                >
                  <Overview onOpenHistory={() => {}} onOpenSettings={() => {}} />
                </main>
              </div>
            </TooltipProvider>
          </InteractionProvider>
        </HotkeySettingsProvider>
      ) : (
        <Fixture />
      )}
    </AppViewport>
  </I18nextProvider>,
);
