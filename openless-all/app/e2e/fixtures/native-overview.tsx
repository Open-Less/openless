import { emit } from '@tauri-apps/api/event';
import { mockIPC } from '@tauri-apps/api/mocks';
import { createInstance } from 'i18next';
import { createRoot } from 'react-dom/client';
import { I18nextProvider } from 'react-i18next';
import { AppViewport } from '../../src/ui/layout/AppViewport';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';

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
      case 'get_activity_stats': {
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

const { useActivityStats } = await import('../../src/data/history');
const { ActivityHeatmap } = await import('../../src/features/overview/ActivityHeatmap');
const i18n = createInstance();
await i18n.init({
  lng: 'en',
  resources: {
    en: {
      translation: {
        overview: {
          activityTitle: 'Annual activity',
          activityCount: '{{count}} dictations',
          activityAppsShow: 'Show top apps',
        },
      },
    },
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
createRoot(document.getElementById('root')!).render(
  <I18nextProvider i18n={i18n}>
    <AppViewport scale={false}>
      <Fixture />
    </AppViewport>
  </I18nextProvider>,
);
