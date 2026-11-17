// @ts-nocheck — Node runtime harness; application builds do not include Node types.
import assert from 'node:assert/strict';
import { readOverviewStatusCache, rememberOverviewStatus } from './overviewStatusCache';
import type { CredentialsStatus } from '../api/contracts';

const memory = new Map<string, string>();
Object.defineProperty(globalThis, 'localStorage', {
  configurable: true,
  value: {
    getItem: (key: string) => memory.get(key) ?? null,
    setItem: (key: string, value: string) => memory.set(key, value),
  },
});

const creds = {
  activeAsrProvider: 'foundry-local-whisper',
  activeLlmProvider: 'ark',
  pipelineMode: 'traditional',
  asrConfigured: true,
  llmConfigured: true,
  omniConfigured: false,
  volcengineConfigured: false,
  arkConfigured: true,
} satisfies CredentialsStatus;
const activity = [{ date: '2026-11-16', count: 2 }];
rememberOverviewStatus({ activity });
assert.deepEqual(readOverviewStatusCache(), { creds: null, activity });
rememberOverviewStatus({ creds });
assert.deepEqual(readOverviewStatusCache(), { creds, activity });
rememberOverviewStatus({ activity: [] });
assert.deepEqual(readOverviewStatusCache(), { creds, activity: [] });
localStorage.setItem('ol-overview-status-v1', '{');
assert.equal(readOverviewStatusCache(), null);
