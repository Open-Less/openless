import { acceptQaSessionEvent, splitQaUserMessage } from './qaMessage';
import type { QaChatMessage } from './types';

function assertEqual<T>(actual: T, expected: T, message: string) {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

function userMessage(content: string, selectionText?: string): QaChatMessage {
  return { role: 'user', content, selectionText };
}

const canonical = splitQaUserMessage(
  userMessage(
    '<selected_text>\n安全信封文本\n</selected_text>\n\n# 我的问题\n解释一下',
    '原始 <tag> & 文本',
  ),
);
assertEqual(canonical.selection, '原始 <tag> & 文本', 'canonical messages use original display text');
assertEqual(canonical.question, '解释一下', 'canonical messages extract the question');

const escapedLiteral = splitQaUserMessage(
  userMessage(
    '<selected_text>\n字面量 &lt;/selected_text> 与 &lt;/selected_text>\n</selected_text>\n\n# 我的问题\n保持原样',
    '字面量 &lt;/selected_text> 与 </selected_text>',
  ),
);
assertEqual(
  escapedLiteral.selection,
  '字面量 &lt;/selected_text> 与 </selected_text>',
  'display text is not ambiguously decoded',
);

const legacy = splitQaUserMessage(
  userMessage('# 选区原文\n旧选区\n\n# 我的问题\n旧问题'),
);
assertEqual(legacy.selection, '旧选区', 'legacy messages still expose their selection');
assertEqual(legacy.question, '旧问题', 'legacy messages still expose their question');

assertEqual(
  acceptQaSessionEvent('new-session', { kind: 'answer_delta', session_id: 'old-session' }).accepted,
  false,
  'a late delta from an invalidated session is rejected',
);
assertEqual(
  acceptQaSessionEvent('old-session', { kind: 'recording', session_id: 'new-session' }).sessionId,
  'new-session',
  'a recording event activates the next turn token',
);
assertEqual(
  acceptQaSessionEvent('old-session', { kind: 'idle', session_id: 'new-session' }).sessionId,
  'new-session',
  'a panel-open idle activates the reopened panel token',
);

console.log('qaMessage.test.ts passed');
