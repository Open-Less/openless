// markdown.tsx — 助手 markdown 正文（原 QaPanel/LessComputerPanel 的
// AssistantText 去重合并）：帧率节流 + 渲染失败降级纯文本 + DOMPurify 兜底
// 消毒，流式时末尾带脉冲光标。样式 .olchat-answer（chat.css）。

import { useMemo } from 'react';
import DOMPurify from 'dompurify';
import { useRafThrottle } from '../../lib/useRafThrottle';
import { renderQaMarkdown, renderQaPlainText } from '../../lib/qaMarkdown';
import './chat.css';

interface AssistantMarkdownProps {
  markdown: string;
  streaming?: boolean;
}

export function AssistantMarkdown({ markdown, streaming = false }: AssistantMarkdownProps) {
  // 按帧率节流：流式回复逐 token 全量 parse + DOMPurify 是 O(n²)，长回复越来越卡。
  const throttled = useRafThrottle(markdown);
  const html = useMemo(() => {
    let rendered: string;
    try {
      rendered = renderQaMarkdown(throttled);
    } catch (error) {
      console.error('[chat] markdown render failed', error);
      rendered = renderQaPlainText(String(throttled ?? ''));
    }
    // 兜底再消毒：qaMarkdown 已转义 raw HTML token，DOMPurify 多一道防线。
    return DOMPurify.sanitize(rendered, { ADD_ATTR: ['target', 'rel'] });
  }, [throttled]);
  return (
    <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'stretch', gap: 3 }}>
      <div
        className="olchat-answer"
        // eslint-disable-next-line react/no-danger
        dangerouslySetInnerHTML={{ __html: html }}
      />
      {streaming && <span className="olchat-caret" />}
    </div>
  );
}
