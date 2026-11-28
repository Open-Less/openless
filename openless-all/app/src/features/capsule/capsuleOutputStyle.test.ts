import type { CapsuleStyleMenuRequest, DictationStyleTarget, StylePack } from '../../api/contracts';
import {
  capsuleOutputStyleLabel,
  CapsuleStylePickerRequests,
  forgetStylePickerRequest,
  projectCapsuleOutputStyle,
  sameCapsuleOutputStyle,
  styleMenuKeyAction,
  stylePickerRequestForSession,
} from './capsuleOutputStyle';

function expect(actual: unknown, expected: unknown) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`Expected ${JSON.stringify(expected)}, received ${JSON.stringify(actual)}`);
  }
}

const request = { requestId: '13', sessionId: 'recording-a' };
const frame = {
  state: 'recording',
  stylePackId: 'builtin.formal',
  stylePackName: '正式表达',
  skipAutoStyle: true,
} as const;
const style = { packId: 'builtin.formal', name: '正式表达' };

expect(projectCapsuleOutputStyle({ ...frame, skipAutoStyle: false }), null);
expect(projectCapsuleOutputStyle({ ...frame, state: 'idle' }), null);
expect(projectCapsuleOutputStyle({ ...frame, selectionPolish: true }), null);
expect(projectCapsuleOutputStyle({ ...frame, qa: true }), null);
expect(projectCapsuleOutputStyle(frame), style);
expect(projectCapsuleOutputStyle({ ...frame, state: 'polishing' }), style);
expect(sameCapsuleOutputStyle(projectCapsuleOutputStyle(frame), style), true);
expect(sameCapsuleOutputStyle(projectCapsuleOutputStyle(frame), null), false);

const labels = { raw: 'Verbatim', light: 'Light', structured: 'Structured', formal: 'Formal' };
expect(capsuleOutputStyleLabel(style, labels), 'Formal');
expect(capsuleOutputStyleLabel({ ...style, name: '  My writing  ' }, labels), 'My writing');
expect(capsuleOutputStyleLabel({ ...style, name: null }, labels), 'Formal');
expect(capsuleOutputStyleLabel({ packId: 'installed-pack-id', name: null }, labels), null);

expect(stylePickerRequestForSession(request, request.sessionId, false), null);
expect(stylePickerRequestForSession(request, undefined, true), null);
expect(stylePickerRequestForSession(request, 'wrong-recording', true), null);
expect(stylePickerRequestForSession(null, request.sessionId, true), null);
expect(stylePickerRequestForSession(request, request.sessionId, true), request);

const pack: StylePack = {
  id: 'builtin.formal',
  name: '正式表达',
  description: '',
  kind: 'builtin',
  baseMode: 'formal',
  version: '1.0.0',
  selectionPrompt: '',
  voiceEditPrompt: '',
  prompt: '',
  examples: [],
  tags: [],
  enabled: true,
  active: true,
};
const packs: StylePack[] = [
  { ...pack, id: 'installed', name: 'Editorial', kind: 'imported' },
  pack,
  { ...pack, id: 'builtin.raw', name: '原文', baseMode: 'raw' },
];
const target: DictationStyleTarget = {
  sessionId: request.sessionId,
  selection: { mode: 'skipAuto' },
  skipAutoStyle: true,
  stylePackId: 'installed',
  stylePackName: 'Editorial',
  polishStylePackId: 'builtin.formal',
  availableStyles: packs,
  mutable: true,
};
const requests = new CapsuleStylePickerRequests();
const loadedRequest = { ...request, target };
const newerLoadedRequest = { ...loadedRequest, requestId: '14' };
const otherSessionRequest = { ...request, sessionId: 'recording-b' };
expect(forgetStylePickerRequest(request, request), null);
expect(forgetStylePickerRequest(loadedRequest, request), null);
expect(forgetStylePickerRequest(newerLoadedRequest, request), newerLoadedRequest);
expect(forgetStylePickerRequest(otherSessionRequest, request), otherSessionRequest);
expect(requests.take(request, { ...target, mutable: false }), null);
expect(requests.take(request, { ...target, sessionId: 'old-recording' }), null);
expect(requests.take(request, { ...target, selection: { mode: 'automatic' } }), null);
expect(requests.take(request, target), 'builtin.formal');
expect(requests.take(request, target), null);
expect(requests.take({ ...request, requestId: '14' }, target), 'builtin.formal');
expect(
  requests.take({ ...request, requestId: '15' }, { ...target, polishStylePackId: null }),
  'installed',
);
expect(requests.take(request, target), null);

const successor: CapsuleStyleMenuRequest = { requestId: '2', sessionId: 'recording-b' };
expect(requests.take(successor, { ...target, sessionId: successor.sessionId }), 'builtin.formal');

expect(styleMenuKeyAction({ key: 'ArrowDown', repeat: true }, packs, 'builtin.formal'), {
  type: 'highlight',
  stylePackId: 'builtin.raw',
});
expect(styleMenuKeyAction({ key: 'ArrowUp', repeat: false }, packs, 'installed'), {
  type: 'highlight',
  stylePackId: 'builtin.raw',
});
expect(styleMenuKeyAction({ key: 'Enter', repeat: false }, packs, 'installed'), {
  type: 'select',
  stylePackId: 'installed',
});
expect(styleMenuKeyAction({ key: 'Escape', repeat: false }, [], ''), { type: 'close' });
expect(styleMenuKeyAction({ key: 'Enter', repeat: false }, packs, 'removed'), undefined);
expect(styleMenuKeyAction({ key: 'Enter', repeat: true }, packs, 'installed'), undefined);
expect(styleMenuKeyAction({ key: 'Escape', repeat: true }, [], ''), undefined);
