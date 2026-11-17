import {
  overviewFollowsBackendEvent,
  readOverviewStatusCache,
  writeOverviewStatusCache,
} from './overviewStatusCache';
import type { CredentialsStatus } from './types';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

const memory = new Map<string, string>();
Object.defineProperty(globalThis, 'localStorage', {
  configurable: true,
  value: {
    getItem: (key: string) => memory.get(key) ?? null,
    setItem: (key: string, value: string) => memory.set(key, value),
  },
});

const creds: CredentialsStatus = {
  activeAsrProvider: 'foundry-local-whisper',
  activeLlmProvider: 'ark',
  pipelineMode: 'traditional',
  asrConfigured: true,
  llmConfigured: true,
  omniConfigured: false,
  volcengineConfigured: false,
  arkConfigured: true,
};

assert(
  !overviewFollowsBackendEvent('dictation_completed'),
  'completion is not yet a stored history row',
);
assert(overviewFollowsBackendEvent('history_changed'), 'a stored history change refreshes overview');
assert(!overviewFollowsBackendEvent('prefs_changed'), 'unrelated events do not refresh overview');
assert(!overviewFollowsBackendEvent(undefined), 'a missing event type does not refresh overview');

writeOverviewStatusCache({ creds, activity: [{ date: '2026-11-16', count: 2 }] });
const cached = readOverviewStatusCache();
assert(cached?.creds.activeAsrProvider === 'foundry-local-whisper', 'cached status round-trips');
assert(cached?.activity?.[0]?.count === 2, 'cached activity round-trips');

localStorage.setItem('ol-overview-status-v1', '{');
assert(readOverviewStatusCache() === null, 'a broken cache is ignored');

console.log('overview status cache passed');
