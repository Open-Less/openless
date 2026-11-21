import type { AskClient, AskEvent, AskSnapshot, AskSource, AskSubmission, AskTool, AskTurn } from './client';

function upsertSource(sources: AskSource[], source: AskSource) {
  const index = sources.findIndex((item) => item.sourceId === source.sourceId);
  return index < 0
    ? [...sources, source]
    : sources.map((item, offset) => (offset === index ? source : item));
}

function upsertTool(tools: AskTool[] = [], event: AskTool): AskTool[] {
  const tool = { toolCallId: event.toolCallId, name: event.name, status: event.status, error: event.error };
  const previous = tools.find((item) => item.toolCallId === tool.toolCallId);
  // A delayed running event cannot restart an already finished tool call.
  if (previous && previous.status !== 'running') return tools;
  return previous ? tools.map((item) => item === previous ? tool : item) : [...tools, tool];
}

function stopTools(tools: AskTool[] | undefined, status: string): AskTool[] | undefined {
  if (status !== 'cancelled' && status !== 'failed') return tools;
  return tools?.map((tool) => tool.status === 'running' ? { ...tool, status } : tool);
}

function applySubmission(
  submission: AskSubmission,
  event: Extract<AskEvent, { turnId: string }>,
): AskSubmission {
  switch (event.type) {
    case 'routeResolved':
      return { ...submission, route: event.route };
    case 'turnStatus':
      return { ...submission, status: event.status };
    case 'searchChanged':
      return {
        ...submission,
        searchStatus: event.status,
        sources: event.sources ?? submission.sources,
        searchError: event.error,
      };
    case 'toolChanged':
      return { ...submission, tools: upsertTool(submission.tools, event) };
    case 'sourceChanged':
      return { ...submission, sources: upsertSource(submission.sources, event.source) };
    case 'turnFinished':
      return { ...submission, status: event.status, error: event.error, tools: stopTools(submission.tools, event.status) };
    default:
      return submission;
  }
}

export function applyEvent(snapshot: AskSnapshot, event: AskEvent): AskSnapshot | null {
  if (event.conversationId !== snapshot.conversationId || event.sequence <= snapshot.sequence) {
    return snapshot;
  }
  if (event.sequence !== snapshot.sequence + 1) return null;
  const next = { ...snapshot, sequence: event.sequence };
  const eventTurn = 'turnId' in event ? event.turnId
    : 'submission' in event ? event.submission.currentTurnId
    : event.type === 'turnAccepted' ? event.turn.turnId : undefined;
  const owner = snapshot.submission?.currentTurnId === eventTurn ? snapshot.submission
    : snapshot.turns.find((turn) => turn.turnId === eventTurn);
  if (owner?.clientRequestId && event.clientRequestId && owner.clientRequestId !== event.clientRequestId) return next;
  function consumeAttachments(attachmentIds: string[]) {
    const consumed = new Set(attachmentIds);
    next.attachments = next.attachments.filter((item) => !consumed.has(item.attachmentId));
  }
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
    case 'submitted':
      if (next.submission?.currentTurnId === event.submission.currentTurnId || next.turns.some((turn) => turn.turnId === event.submission.currentTurnId)) return next;
      if (next.activeTurnId) return null;
      next.activeTurnId = event.submission.currentTurnId;
      next.submission = { ...event.submission, clientRequestId: event.submission.clientRequestId ?? event.clientRequestId };
      if (event.attachmentIds) consumeAttachments(event.attachmentIds);
      return next;
    case 'delivery':
      if (next.submission?.currentTurnId === event.submission.currentTurnId) {
        if (next.submission.status === 'cancelled') return next;
        const delivery = { ...event.submission,
          clientRequestId: event.submission.clientRequestId ?? next.submission.clientRequestId,
          tools: event.submission.tools ?? next.submission.tools,
        };
        next.submission = next.activeTurnId === event.submission.currentTurnId ? delivery : {
          ...delivery, status: next.submission.status, error: next.submission.error,
          tools: stopTools(delivery.tools, next.submission.status),
        };
      }
      return next;
    case 'turnAccepted': {
      if (next.turns.some((turn) => turn.turnId === event.turn.turnId)) return next;
      if (next.submission?.currentTurnId === event.turn.turnId && !next.activeTurnId) return next;
      if (next.activeTurnId && next.activeTurnId !== event.turn.turnId) return null;
      next.turns = [...next.turns, { ...event.turn, clientRequestId: event.turn.clientRequestId ?? event.clientRequestId ?? owner?.clientRequestId }];
      next.activeTurnId = event.turn.turnId;
      return next;
    }
    default: {
      if (next.activeTurnId !== event.turnId) return next;
      const submission = next.submission?.currentTurnId === event.turnId ? next.submission : null;
      if (submission) next.submission = applySubmission(submission, event);
      if (event.type === 'turnFinished') next.activeTurnId = null;
      if (event.type === 'routeResolved' || event.type === 'usageChanged') return next;
      const turn = next.turns.find((item) => item.turnId === event.turnId);
      if (!turn) return submission && event.type !== 'answerDelta' ? next : null;
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
        case 'toolChanged':
          updated = { ...turn, tools: upsertTool(turn.tools, event) };
          break;
        case 'sourceChanged':
          updated = {
            ...turn,
            sources: upsertSource(turn.sources, event.source),
          };
          break;
        case 'turnFinished':
          updated = {
            ...turn,
            status: event.status,
            error: event.error,
            tools: stopTools(turn.tools, event.status),
            assistantMessage: event.status === 'completed' && typeof event.text === 'string'
              ? { messageId: turn.assistantMessage?.messageId ?? event.turnId, text: event.text }
              : turn.assistantMessage,
          };
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
      if (event.type === 'submitted' && !event.attachmentIds && snapshot.attachments.length) break;
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
