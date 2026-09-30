import type { QaChatMessage, QaStatePayload } from './types';

export function splitQaUserMessage(message: QaChatMessage): {
  selection: string;
  question: string;
} {
  const parsed = splitQaUserContent(message.content);
  return {
    selection: message.selectionText ?? parsed.selection,
    question: parsed.question,
  };
}

function splitQaUserContent(content: string): { selection: string; question: string } {
  const envelope = content.match(
    /^<selected_text>\n([\s\S]*?)\n<\/selected_text>\n\n# 我的问题\n([\s\S]+)$/,
  );
  if (envelope) {
    return { selection: envelope[1].trim(), question: envelope[2].trim() };
  }

  // Compatibility with the old format already saved in the current session before the fix.
  const legacy = content.match(/^# 选区原文\n([\s\S]*?)\n\n# 我的问题\n([\s\S]+)$/);
  if (legacy) {
    return { selection: legacy[1].trim(), question: legacy[2].trim() };
  }
  return { selection: '', question: content };
}

export function acceptQaSessionEvent(
  currentSessionId: string | null,
  payload: Pick<QaStatePayload, 'kind' | 'sessionId'>,
): { accepted: boolean; sessionId: string | null } {
  if (!payload.sessionId) {
    return { accepted: true, sessionId: currentSessionId };
  }
  // idle is always treated as a new-session token: open_qa_panel's idle always carries a newly
  // generated session_id, and events arrive in send order, so the idle that closes
  // complete/turn always precedes the next open.
  const startsTurn =
    payload.kind === 'recording' ||
    payload.kind === 'loading' ||
    payload.kind === 'thinking' ||
    payload.kind === 'idle';
  if (currentSessionId && !startsTurn && currentSessionId !== payload.sessionId) {
    return { accepted: false, sessionId: currentSessionId };
  }
  return {
    accepted: true,
    sessionId: !currentSessionId || startsTurn ? payload.sessionId : currentSessionId,
  };
}
