import { createRoot } from 'react-dom/client';
import { AskWindow } from '../../ask-window/ask-window';
import type {
  AskClient,
  AskEditor,
  AskEvent,
  AskFocus,
  AskRoute,
  AskSnapshot,
  AskSubmission,
  AskTurn,
  AskVoiceEvent,
  AskWindowEvent,
  AttachmentInfo,
  SubmitAsk,
} from '../../ask-window/client';
import { applyEvent } from '../../ask-window/feed';

type PendingEvent = AskEvent extends infer Event
  ? Event extends AskEvent
    ? Omit<Event, 'conversationId' | 'sequence'>
    : never
  : never;

const image: AttachmentInfo = {
  attachmentId: 'image',
  kind: 'image',
  displayName: 'image.png',
  state: 'ready',
  preview: { kind: 'image', width: 1, height: 1, thumbnailAvailable: true },
};
const imageBase64 =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aKI8AAAAASUVORK5CYII=';

const pdf: AttachmentInfo = {
  attachmentId: 'pdf',
  kind: 'pdf',
  displayName: 'a notes.pdf',
  state: 'ready',
  pageCount: 3,
};

const excerpt: AttachmentInfo = {
  attachmentId: 'selection',
  kind: 'selection',
  state: 'ready',
  displayName: 'Selected text',
  sourceApp: 'TextEdit',
  preview: { kind: 'selection', text: 'Private excerpt.', hasMore: true },
};

const handlers = {
  event: new Set<(event: AskEvent) => void>(),
  window: new Set<(event: AskWindowEvent) => void>(),
  voice: new Set<(event: AskVoiceEvent) => void>(),
};

let current: AskSnapshot = {
  conversationId: 'conversation-1',
  sequence: 0,
  turns: [],
  attachments: [],
  activeTurnId: null,
};
let events: AskEvent[] = [];
let focus: AskFocus = { focusEpoch: 1, focusRequest: 1 };
let lastEditor: AskEditor | undefined;
const voiceScope = new Map<string, Omit<AskVoiceEvent, 'type'>>();
const delivered = new Set<string>();
function push(event: PendingEvent, live = true, replayable = true) {
  const packet = {
    ...event,
    sequence: current.sequence + 1,
    conversationId: current.conversationId,
  };
  const updated = applyEvent(current, packet);
  if (!updated) throw new Error(`Fixture event is invalid: ${packet.type}`);
  current = updated;
  if (replayable) events.push(packet);
  if (live) handlers.event.forEach((listener) => listener(packet));
}

function resolveRoute(route: AskRoute, request: SubmitAsk, live = true) {
  const turnId = current.activeTurnId!;
  push({ type: 'routeResolved', route, turnId }, live);
  if (route !== 'answer' && route !== 'selectionAnswer') return;
  const turn: AskTurn = {
    turnId,
    userMessage: { messageId: `${turnId}-input`, text: request.text },
    attachments: attachmentArchive.filter((item) =>
      request.attachmentIds.includes(item.attachmentId),
    ),
    deepThinking: request.options.deepThinking,
    status: 'queued',
    searchStatus: 'idle',
    sources: [],
  };
  push({ type: 'turnAccepted', turn });
}

function reset(id: string) {
  current = { conversationId: id, sequence: 0, turns: [], attachments: [], activeTurnId: null };
  events = [];
}

function subscribe<Event>(listeners: Set<(event: Event) => void>, receive: (event: Event) => void) {
  listeners.add(receive);
  return Promise.resolve(() => {
    listeners.delete(receive);
  });
}

const fixture = {
  submits: [] as SubmitAsk[],
  opened: [] as [string, string, string][],
  drops: [] as { conversationId: string; displayName: string; dataBase64: string }[],
  imageBase64,
  cancelled: [] as [string, string][],
  hideCount: 0,
  expandCount: 0,
  snapshotReads: 0,
  rejectNext: false,
  route: 'answer' as AskRoute | null,
  truncate: false,
  liveListeners: () => handlers.event.size,
  voiceSelection: () =>
    lastEditor?.editable ? [lastEditor.selectionStart, lastEditor.selectionEnd] : null,
  active: () => current.activeTurnId!,
  push,
  resolve(route: AskRoute, live = true) {
    resolveRoute(route, fixture.submits.at(-1)!, live);
  },
  addInitialCards() {
    push({ type: 'attachmentChanged', attachment: image });
    push({ type: 'attachmentChanged', attachment: pdf });
    push({ type: 'attachmentChanged', attachment: excerpt });
  },
  activate() {
    focus = { ...focus, focusRequest: focus.focusRequest + 1 };
    handlers.window.forEach((listener) => listener({ type: 'activated', ...focus }));
  },
  voice(
    event:
      | { type: 'transcribed'; recordingId: string; text: string }
      | { type: 'started' | 'cancelled'; recordingId: string },
  ) {
    if (event.type === 'started' && lastEditor?.editable) {
      voiceScope.set(event.recordingId, {
        ...lastEditor,
        ...focus,
        recordingId: event.recordingId,
      });
    }
    const target = voiceScope.get(event.recordingId);
    if (target)
      handlers.voice.forEach((listener) => listener({ ...target, ...event } as AskVoiceEvent));
  },
  switch() {
    reset('conversation-2');
    lastEditor = undefined;
    focus = { focusEpoch: focus.focusEpoch + 1, focusRequest: focus.focusRequest };
    handlers.window.forEach((listener) => listener({ type: 'sessionChanged', ...focus }));
  },
};

const accepted = new Map<string, string>();
let attachmentArchive: AttachmentInfo[] = [];
const client: AskClient = {
  async current() {
    return { conversationId: current.conversationId, ...focus };
  },
  async newConversation() {
    reset('conversation-2');
    return { conversationId: current.conversationId };
  },
  async endConversation() {},
  async snapshot() {
    fixture.snapshotReads++;
    return current;
  },
  async events(_conversationId, afterSequence) {
    return {
      events: events.filter((event) => event.sequence > afterSequence),
      latestSequence: current.sequence,
      truncated: fixture.truncate,
    };
  },
  async submit(request) {
    fixture.submits.push(request);
    if (fixture.rejectNext) {
      fixture.rejectNext = false;
      throw { code: 'not_configured', message: 'Set up an Ask model first.' };
    }
    const previous = accepted.get(request.clientRequestId);
    if (previous) return { turnId: previous };
    const turnId = `turn-${fixture.submits.length}`;
    const submission: AskSubmission = {
      currentTurnId: turnId,
      status: 'queued',
      searchStatus: 'idle',
      sources: [],
      copyStatus: 'notNeeded',
    };
    attachmentArchive = current.attachments;
    accepted.set(request.clientRequestId, turnId);
    push({ type: 'submitted', submission, attachmentIds: request.attachmentIds });
    if (fixture.route) resolveRoute(fixture.route, request);
    return { turnId };
  },
  async cancel(conversationId, turnId) {
    fixture.cancelled.push([conversationId, turnId]);
    push({ type: 'turnFinished', turnId, status: 'cancelled' });
  },
  async addAttachment() {
    push({ type: 'attachmentChanged', attachment: pdf });
    return pdf;
  },
  async addDroppedImage(conversationId, displayName, dataBase64) {
    fixture.drops.push({ conversationId, displayName, dataBase64 });
    const attachment = { ...image, attachmentId: 'dropped-image', displayName };
    push({ type: 'attachmentChanged', attachment });
    return attachment;
  },
  async removeAttachment(_conversationId, attachmentId) {
    push({ type: 'attachmentRemoved', attachmentId });
  },
  async getAttachmentPreview(_conversationId, attachmentId) {
    return attachmentId === 'image' || attachmentId === 'dropped-image'
      ? {
          kind: 'image',
          mimeType: 'image/png',
          dataBase64: imageBase64,
        }
      : { kind: 'selection', text: 'Private excerpt. And the exact remaining selected text.' };
  },
  async openSource(conversationId, turnId, sourceId) {
    fixture.opened.push([conversationId, turnId, sourceId]);
  },
  async hide() {
    focus = { ...focus, focusEpoch: focus.focusEpoch + 1 };
    fixture.hideCount++;
    handlers.window.forEach((listener) => listener({ type: 'hidden' }));
  },
  async expandChat() {
    fixture.expandCount++;
  },
  async syncEditor(scope, editor) {
    if (scope.focusEpoch !== focus.focusEpoch || scope.focusRequest !== focus.focusRequest)
      return false;
    lastEditor = editor;
    return true;
  },
  async acknowledgeVoice(id) {
    const target = voiceScope.get(id);
    if (
      delivered.has(id) ||
      !target ||
      target.focusEpoch !== focus.focusEpoch ||
      target.revision !== lastEditor?.revision ||
      target.mountId !== lastEditor?.mountId
    )
      return false;
    delivered.add(id);
    return true;
  },
  onEvent: (receive) => subscribe(handlers.event, receive),
  onWindowEvent: (receive) => subscribe(handlers.window, receive),
  onVoiceEvent: (receive) => subscribe(handlers.voice, receive),
};

declare global {
  interface Window {
    askFixture: typeof fixture;
  }
}

window.askFixture = fixture;
createRoot(document.querySelector('#ask-root')!).render(<AskWindow client={client} />);
