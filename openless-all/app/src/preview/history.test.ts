// @ts-nocheck — Node runtime harness; application builds do not include Node types.
import assert from 'node:assert/strict';
import { clearServiceUsage, getServiceUsage } from '../api/usage';
import {
  clearHistory,
  deleteHistoryEntry,
  applyQuickNoteRepolish,
  readAudioRecording,
} from '../api/history';
import { startDictation, stopDictation, cancelDictation } from '../api/dictation';
import { getSettingsSnapshot, listenPreferenceChanges, setSettings } from '../api/preferences';
import { historyStore } from '../data/history';
import { listenPreviewChange } from './events';
import { previewHistory } from './history';

const year = new Date().getFullYear();
const [records, activity, apps, usage] = await Promise.all([
  historyStore.refreshRecords(),
  historyStore.refreshActivity(),
  historyStore.refreshApps(year),
  getServiceUsage(),
]);
assert.ok(records.data);
const ordinary = records.data.find((entry) => entry.source === 'voice');
const note = records.data.find((entry) => entry.source === 'quick_note');
assert.ok(ordinary && note && activity.data?.length && apps.data?.status === 'available');
assert.ok(usage.records.length);
assert.ok((await readAudioRecording(note.id)).startsWith('data:audio/wav;base64,UklGR'));
const independent = previewHistory.list();
independent[0].finalText = 'external mutation';
assert.notEqual(previewHistory.list()[0].finalText, 'external mutation');

let idsAtSignal: string[] | undefined;
const unlisten = listenPreviewChange('history', () => {
  idsAtSignal = previewHistory.list().map((entry) => entry.id);
});
await deleteHistoryEntry(ordinary.id);
assert.ok(idsAtSignal);
assert.ok(!idsAtSignal.includes(ordinary.id), 'preview commit happens before publication');
assert.ok(!(await historyStore.records.load()).data?.some((entry) => entry.id === ordinary.id));

await startDictation();
await assert.rejects(clearHistory(), /recording is active/);
await cancelDictation();
previewHistory.startRecording(note.id);
await assert.rejects(deleteHistoryEntry(note.id), /active recording/);
await assert.rejects(clearHistory(), /recording is active/);
previewHistory.cancelRecording();
await applyQuickNoteRepolish(note.id, 'Updated note');
await clearHistory();
const remaining = (await historyStore.records.load()).data;
assert.deepEqual(
  remaining?.map(({ id, finalText }) => [id, finalText]),
  [[note.id, 'Updated note']],
);
assert.deepEqual((await historyStore.activity.load()).data, activity.data);
assert.deepEqual((await historyStore.apps(year).load()).data, apps.data);
assert.deepEqual(
  await getServiceUsage(),
  usage,
  'history clearing must not affect the service ledger',
);

await startDictation();
await stopDictation();
assert.equal((await historyStore.records.load()).data?.length, 2);
const updatedActivity = (await historyStore.activity.load()).data;
assert.equal(
  updatedActivity?.reduce((count, day) => count + day.count, 0),
  activity.data.reduce((count, day) => count + day.count, 0) + 1,
);
assert.notDeepEqual((await historyStore.apps(year).load()).data, apps.data);
unlisten();
await clearServiceUsage();
assert.deepEqual(await getServiceUsage(), { records: [], droppedOrFailed: 0 });
assert.equal((await historyStore.records.load()).data?.length, 2);

const prior = await getSettingsSnapshot();
let nextRead: ReturnType<typeof getSettingsSnapshot> | undefined;
const stop = await listenPreferenceChanges(() => {
  nextRead = getSettingsSnapshot();
});
await setSettings({ ...prior.preferences, themeMode: 'dark' });
const next = await nextRead;
assert.ok(next && next.revision > prior.revision && next.preferences.themeMode === 'dark');
stop();
console.log(
  'Preview history writes, recording protection, aggregate independence, and prefs revisions passed',
);
