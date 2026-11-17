import type { ActivityDay, DictationSession } from '../api/contracts';
import { previewActivityDays, previewHistoryEntries } from './fixtures';
import { publishPreviewChange } from './events';

type ActivityApp = [name: string, count: number];

export function createPreviewHistory(
  initialEntries: readonly DictationSession[],
  initialDays: readonly ActivityDay[],
  changed: () => void,
) {
  let entries = initialEntries.map((entry) => ({ ...entry }));
  const activity = new Map(initialDays.map((day) => [day.date, { ...day }]));
  const apps = new Map<number, Map<string, number>>();
  let active: string | null = null;
  let nextId = 0;

  const addAppActivity = (year: number, name: string, count: number) => {
    if (!count) return;
    let rows = apps.get(year);
    if (!rows) {
      rows = new Map();
      apps.set(year, rows);
    }
    rows.set(name, (rows.get(name) ?? 0) + count);
  };
  for (const day of initialDays) {
    const year = Number(day.date.slice(0, 4));
    const inEditor = Math.ceil(day.count / 2);
    const inBrowser = Math.floor((day.count - inEditor) / 2);
    addAppActivity(year, 'VS Code', inEditor);
    addAppActivity(year, 'Browser', inBrowser);
    addAppActivity(year, 'Notes', day.count - inEditor - inBrowser);
  }

  const entryFor = (id: string) => {
    const entry = entries.find((entry) => entry.id === id);
    if (!entry) throw new Error('history entry not found');
    return entry;
  };
  const replaceEntry = (entry: DictationSession) => {
    entries = entries.map((previous) => (previous.id === entry.id ? entry : previous));
    changed();
    return { ...entry };
  };

  return {
    list: () => entries.map((entry) => ({ ...entry })),
    getActivityDays: () =>
      [...activity.values()]
        .sort((a, b) => a.date.localeCompare(b.date))
        .map((day) => ({ ...day })),
    getActivityApps: (year: number): ActivityApp[] =>
      [...(apps.get(year) ?? [])].sort((a, b) => b[1] - a[1]),
    delete(id: string) {
      if (active === id)
        throw new Error('cannot delete the active recording; stop or cancel it first');
      entries = entries.filter((entry) => entry.id !== id);
      changed();
    },
    clear() {
      if (active !== null) throw new Error('cannot clear history while a recording is active');
      entries = entries.filter((entry) => entry.source === 'quick_note');
      changed();
    },
    readRecording(id: string) {
      if (entryFor(id).hasAudioRecording !== true) throw new Error('recording not found');
      return previewRecording();
    },
    retranscribe(id: string) {
      const entry = entryFor(id);
      if (active === id)
        throw new Error('recording is still active; stop it before retranscribing');
      if (!entry.hasAudioRecording) throw new Error('history entry has no archived recording');
      if (entry.pipelineMode === 'multimodal' && entry.source !== 'quick_note') {
        throw new Error('multimodal history does not support retranscription');
      }
      const text = entry.rawTranscript || 'Preview transcription';
      const shouldRepair =
        entry.source === 'quick_note' ||
        ['transcribeFailed', 'emptyTranscript', 'recording'].includes(entry.errorCode ?? '');
      return {
        text,
        updatedEntry: shouldRepair
          ? replaceEntry({
              ...entry,
              rawTranscript: text,
              finalText: text,
              errorCode: null,
              llmProvider: null,
              llmModel: null,
              polishMs: null,
            })
          : null,
      };
    },
    repolishQuickNote(id: string, text: string, stylePackId?: string) {
      if (!text.trim()) throw new Error('repolish text is empty');
      const entry = entryFor(id);
      if (entry.source !== 'quick_note')
        throw new Error('only quick-note history can apply repolish results');
      return replaceEntry({
        ...entry,
        finalText: text,
        errorCode: null,
        stylePackId: stylePackId ?? entry.stylePackId,
      });
    },
    startRecording(id = `preview-recording-${++nextId}`) {
      if (active !== null) throw new Error('a recording is already active');
      active = id;
      return id;
    },
    cancelRecording() {
      active = null;
    },
    stopRecording() {
      if (active === null) throw new Error('there is no active recording');
      const id = active;
      active = null;
      const template = initialEntries.find((entry) => entry.source !== 'quick_note');
      if (!template) return;
      const now = new Date();
      const createdAt = now.toISOString();
      entries = [
        { ...template, id, createdAt, source: 'voice' },
        ...entries.filter((entry) => entry.id !== id),
      ];
      const date = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
      const previous = activity.get(date);
      activity.set(date, {
        date,
        count: (previous?.count ?? 0) + 1,
        chars: (previous?.chars ?? 0) + Array.from(template.finalText).length,
        durationMs: (previous?.durationMs ?? 0) + (template.durationMs ?? 0),
      });
      if (template.appName) addAppActivity(now.getFullYear(), template.appName, 1);
      changed();
    },
  };
}

export const previewHistory = createPreviewHistory(previewHistoryEntries, previewActivityDays, () =>
  publishPreviewChange('history'),
);

function previewRecording(): string {
  const sampleRate = 8000;
  const frames = 1600;
  const bytes = new Uint8Array(44 + frames * 2);
  const view = new DataView(bytes.buffer);
  const label = (offset: number, value: string) => {
    for (let index = 0; index < value.length; index++)
      bytes[offset + index] = value.charCodeAt(index);
  };
  label(0, 'RIFF');
  view.setUint32(4, bytes.length - 8, true);
  label(8, 'WAVEfmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  label(36, 'data');
  view.setUint32(40, frames * 2, true);
  for (let frame = 0; frame < frames; frame++) {
    view.setInt16(44 + frame * 2, Math.sin((2 * Math.PI * frame * 440) / sampleRate) * 1000, true);
  }
  return 'data:audio/wav;base64,' + btoa(String.fromCharCode(...bytes));
}
