import type { QaChatMessage, QaStatePayload } from './types';

export function splitQaUserMessage(
  message: QaChatMessage,
): { selection: string; question: string } {
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

  // 兼容修复前已保存在当前会话中的旧格式。
  const legacy = content.match(/^# 选区原文\n([\s\S]*?)\n\n# 我的问题\n([\s\S]+)$/);
  if (legacy) {
    return { selection: legacy[1].trim(), question: legacy[2].trim() };
  }
  return { selection: '', question: content };
}

export function nextQaSelectionWarning(
  current: string,
  payload: Pick<QaStatePayload, 'kind' | 'selection_warning'>,
): string {
  if (payload.kind === 'idle' || payload.kind === 'recording') {
    return payload.selection_warning ?? '';
  }
  if (payload.kind === 'loading' || payload.kind === 'thinking') {
    return payload.selection_warning === undefined ? current : (payload.selection_warning ?? '');
  }
  return current;
}

export function acceptQaSessionEvent(
  currentSessionId: string | null,
  payload: Pick<QaStatePayload, 'kind' | 'session_id' | 'selection_warning'>,
): { accepted: boolean; sessionId: string | null } {
  if (!payload.session_id) {
    return { accepted: true, sessionId: currentSessionId };
  }
  const startsTurn = payload.kind === 'recording'
    || payload.kind === 'loading'
    || payload.kind === 'thinking'
    || (payload.kind === 'idle' && payload.selection_warning !== undefined);
  if (currentSessionId && !startsTurn && currentSessionId !== payload.session_id) {
    return { accepted: false, sessionId: currentSessionId };
  }
  return {
    accepted: true,
    sessionId: !currentSessionId || startsTurn ? payload.session_id : currentSessionId,
  };
}
