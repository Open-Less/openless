// markdown.tsx — assistant markdown body (deduped merge of QaPanel/LessComputerPanel's
// AssistantText): frame-throttled, falls back to plain text on render failure, with
// DOMPurify as a final sanitize pass; a pulsing caret trails while streaming.
// Styled by .olchat-answer (chat.css).

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
  // Frame-throttled: full parse + DOMPurify per token while streaming is O(n²), and
  // long replies get progressively jankier.
  const throttled = useRafThrottle(markdown);
  const html = useMemo(() => {
    let rendered: string;
    try {
      rendered = renderQaMarkdown(throttled);
    } catch (error) {
      console.error('[chat] markdown render failed', error);
      rendered = renderQaPlainText(String(throttled ?? ''));
    }
    // Extra sanitize pass: qaMarkdown already escapes raw HTML tokens; DOMPurify adds
    // one more line of defense.
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
