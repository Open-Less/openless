import { createRoot } from 'react-dom/client';
import { AskWindow } from '../../ask-window/ask-window';
import type {
  AskClient,
  AskEvent,
  AskSnapshot,
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
  snapshotReads: 0,
  rejectNext: false,
  truncate: false,
  liveListeners: () => handlers.event.size,
  active: () => current.activeTurnId!,
  push,
  addInitialCards() {
    push({ type: 'attachmentChanged', attachment: image });
    push({ type: 'attachmentChanged', attachment: pdf });
    push({ type: 'attachmentChanged', attachment: excerpt });
  },
  activate() {
    handlers.window.forEach((listener) => listener({ type: 'activated' }));
  },
  voice(
    event:
      | Omit<Extract<AskVoiceEvent, { type: 'transcribed' }>, 'conversationId'>
      | Omit<Extract<AskVoiceEvent, { type: 'started' | 'cancelled' }>, 'conversationId'>,
  ) {
    handlers.voice.forEach((listener) =>
      listener({ ...event, conversationId: current.conversationId }),
    );
  },
  switch() {
    reset('conversation-2');
    fixture.activate();
  },
};

const accepted = new Map<string, string>();
const client: AskClient = {
  async current() {
    return { conversationId: current.conversationId };
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
    const turn: AskTurn = {
      turnId,
      userMessage: { messageId: `${turnId}-input`, text: request.text },
      attachments: current.attachments.filter((attachment) =>
        request.attachmentIds.includes(attachment.attachmentId),
      ),
      deepThinking: request.options.deepThinking,
      status: 'queued',
      searchStatus: 'idle',
      sources: [],
    };
    accepted.set(request.clientRequestId, turnId);
    push({ type: 'turnAccepted', turn });
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
    fixture.hideCount++;
    handlers.window.forEach((listener) => listener({ type: 'hidden' }));
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
