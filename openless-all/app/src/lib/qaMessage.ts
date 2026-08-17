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

export function acceptQaSessionEvent(
  currentSessionId: string | null,
  payload: Pick<QaStatePayload, 'kind' | 'session_id'>,
): { accepted: boolean; sessionId: string | null } {
  if (!payload.session_id) {
    return { accepted: true, sessionId: currentSessionId };
  }
  // idle 一律视为新会话 token：open_qa_panel 的 idle 总是携带新生成的 session_id，
  // 且事件按发送顺序到达，complete/turn 收尾的 idle 一定先于下一次 open。
  const startsTurn = payload.kind === 'recording'
    || payload.kind === 'loading'
    || payload.kind === 'thinking'
    || payload.kind === 'idle';
  if (currentSessionId && !startsTurn && currentSessionId !== payload.session_id) {
    return { accepted: false, sessionId: currentSessionId };
  }
  return {
    accepted: true,
    sessionId: !currentSessionId || startsTurn ? payload.session_id : currentSessionId,
  };
}
