import type { AskClient, AskEvent, AskSnapshot, AskTurn } from './client';

export function applyEvent(snapshot: AskSnapshot, event: AskEvent): AskSnapshot | null {
  if (event.conversationId !== snapshot.conversationId || event.sequence <= snapshot.sequence) {
    return snapshot;
  }
  if (event.sequence !== snapshot.sequence + 1) return null;
  const next = { ...snapshot, sequence: event.sequence };
  switch (event.type) {
    case 'attachmentChanged': {
      const attachment = event.attachment;
      const index = next.attachments.findIndex(
        (item) => item.attachmentId === attachment.attachmentId,
      );
      next.attachments =
        index < 0
          ? [...next.attachments, attachment]
          : next.attachments.map((item, itemIndex) => (itemIndex === index ? attachment : item));
      return next;
    }
    case 'attachmentRemoved':
      next.attachments = next.attachments.filter(
        (item) => item.attachmentId !== event.attachmentId,
      );
      return next;
    case 'turnAccepted': {
      if (next.activeTurnId || next.turns.some((turn) => turn.turnId === event.turn.turnId))
        return null;
      next.turns = [...next.turns, event.turn];
      next.activeTurnId = event.turn.turnId;
      const consumed = new Set(event.turn.attachments.map((item) => item.attachmentId));
      next.attachments = next.attachments.filter((item) => !consumed.has(item.attachmentId));
      return next;
    }
    default: {
      const turn = next.turns.find((item) => item.turnId === event.turnId);
      if (!turn) return null;
      if (next.activeTurnId !== event.turnId) return next;
      let updated: AskTurn;
      switch (event.type) {
        case 'answerDelta':
          if (turn.assistantMessage && turn.assistantMessage.messageId !== event.messageId)
            return null;
          updated = {
            ...turn,
            status: 'writing',
            assistantMessage: {
              messageId: event.messageId,
              text: (turn.assistantMessage?.text ?? '') + event.text,
            },
          };
          break;
        case 'turnStatus':
          updated = { ...turn, status: event.status };
          break;
        case 'searchChanged':
          updated = {
            ...turn,
            searchStatus: event.status,
            sources: event.sources ?? turn.sources,
            searchError: event.error,
          };
          break;
        case 'sourceChanged': {
          const index = turn.sources.findIndex((source) => source.sourceId === event.source.sourceId);
          updated = {
            ...turn,
            sources:
              index < 0
                ? [...turn.sources, event.source]
                : turn.sources.map((source, offset) => (offset === index ? event.source : source)),
          };
          break;
        }
        case 'usageChanged':
          return next;
        case 'turnFinished':
          updated = { ...turn, status: event.status, error: event.error };
          next.activeTurnId = null;
          break;
        default:
          return null;
      }
      next.turns = next.turns.map((item) => (item === turn ? updated : item));
      return next;
    }
  }
}

const BUFFER_LIMIT = 256;

export function connectAskFeed(
  client: Pick<AskClient, 'current' | 'snapshot' | 'events' | 'onEvent'>,
  publish: (snapshot: AskSnapshot | null) => void,
  onError: (error: unknown) => void,
) {
  const buffer = new Map<string, AskEvent>();
  const key = (id: string, sequence: number) => `${id}:${sequence}`;
  let conversationId: string | null | undefined;
  let snapshot: AskSnapshot | undefined;
  let unlisten: (() => void) | undefined;
  let stopped = false;
  let running = false;
  let listening = false;
  let requested = false;
  let replay = false;

  const store = (event: AskEvent) => {
    if (conversationId !== undefined && event.conversationId !== conversationId) return;
    if (snapshot && event.sequence <= snapshot.sequence) return;
    const eventKey = key(event.conversationId, event.sequence);
    if (buffer.size >= BUFFER_LIMIT && !buffer.has(eventKey)) {
      buffer.clear();
      replay = true;
    }
    buffer.set(eventKey, event);
  };

  const discardCovered = () => {
    for (const [eventKey, event] of buffer) {
      if (event.conversationId !== conversationId || event.sequence <= (snapshot?.sequence ?? 0)) {
        buffer.delete(eventKey);
      }
    }
  };

  const drain = () => {
    if (!snapshot) return;
    let eventKey = key(snapshot.conversationId, snapshot.sequence + 1);
    for (let event = buffer.get(eventKey); event; event = buffer.get(eventKey)) {
      const next = applyEvent(snapshot, event);
      if (!next) break;
      snapshot = next;
      buffer.delete(eventKey);
      eventKey = key(snapshot.conversationId, snapshot.sequence + 1);
    }
  };

  const synchronize = async () => {
    if (running || stopped) return;
    running = true;
    try {
      if (!listening) {
        unlisten = await client.onEvent((event) => {
          if (stopped) return;
          store(event);
          check(false);
        });
        if (stopped) {
          unlisten();
          return;
        }
        listening = true;
      }
      if (conversationId === undefined) {
        ({ conversationId } = await client.current());
        discardCovered();
      }
      if (stopped) return;
      if (!conversationId) {
        requested = false;
        publish(null);
        return;
      }
      const id = conversationId;
      const restore = async () => {
        const latest = await client.snapshot(id);
        if (latest.conversationId !== id || latest.sequence < (snapshot?.sequence ?? 0)) {
          throw new Error('Ask returned an outdated conversation snapshot.');
        }
        snapshot = latest;
        discardCovered();
      };
      if (!snapshot) await restore();
      while (requested && !stopped) {
        const checkTail = replay;
        requested = false;
        replay = false;
        drain();
        if (checkTail || buffer.size) {
          const result = await client.events(id, snapshot!.sequence);
          if (stopped) return;
          if (!result.truncated) {
            for (const event of result.events) store(event);
            drain();
          }
          if (result.truncated || buffer.size || result.latestSequence > snapshot!.sequence) {
            await restore();
            drain();
          }
        }
        if (!stopped) publish(snapshot!);
      }
    } catch (error) {
      requested = false;
      replay = false;
      if (!stopped) onError(error);
    } finally {
      running = false;
    }
  };

  function check(checkTail = true) {
    if (stopped) return;
    requested = true;
    replay ||= checkTail;
    void synchronize();
  }

  check(false);
  return {
    check,
    disconnect() {
      stopped = true;
      buffer.clear();
      if (listening) unlisten?.();
    },
  };
}
