// markdown.tsx — assistant markdown body (deduped merge of QaPanel/LessComputerPanel's
// AssistantText): frame-throttled, falls back to plain text on render failure, with
// DOMPurify as a final sanitize pass; new streaming glyphs resolve from blur in 90ms.
// Styled by .olchat-answer (chat.css).

import { useLayoutEffect, useMemo, useRef } from 'react';
import DOMPurify from 'dompurify';
import { useRafThrottle } from '../../lib/useRafThrottle';
import { renderQaMarkdown, renderQaPlainText } from '../../lib/qaMarkdown';
import { useReducedMotion } from '../../lib/motion';
import {
  keepProseTailsTogether,
  mountTextReveals,
  planTextReveal,
  type TextRevealState,
} from '../../lib/assistantText';
import './chat.css';

interface AssistantMarkdownProps {
  markdown: string;
  streaming?: boolean;
}

export function AssistantMarkdown({ markdown, streaming = false }: AssistantMarkdownProps) {
  // Frame-throttled: full parse + DOMPurify per token while streaming is O(n²), and
  // long replies get progressively jankier.
  const throttled = useRafThrottle(markdown);
  // Completion/cancellation must flush the final answer even in a background webview.
  const content = streaming ? throttled : markdown;
  const reducedMotion = useReducedMotion();
  const body = useRef<HTMLDivElement>(null);
  const reveal = useRef<TextRevealState>({ text: '', glyphs: [] });
  const html = useMemo(() => {
    let rendered: string;
    try {
      rendered = renderQaMarkdown(content);
    } catch (error) {
      console.error('[chat] markdown render failed', error);
      rendered = renderQaPlainText(String(content ?? ''));
    }
    // Extra sanitize pass: qaMarkdown already escapes raw HTML tokens; DOMPurify adds
    // one more line of defense.
    return DOMPurify.sanitize(rendered, { ADD_ATTR: ['target', 'rel'] });
  }, [content]);

  useLayoutEffect(() => {
    const element = body.current;
    if (!element) return;
    // This subtree is deliberately DOM-owned. Never decorate unsanitized markdown.
    element.innerHTML = html;
    keepProseTailsTogether(element);
    const now = performance.now();
    reveal.current = planTextReveal(
      reveal.current,
      element.textContent ?? '',
      now,
      streaming && !reducedMotion,
    );
    return mountTextReveals(element, reveal.current.glyphs, now);
  }, [html, streaming, reducedMotion]);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'stretch', gap: 3 }}>
      <div ref={body} className="olchat-answer" />
      {streaming && <span className="olchat-caret" />}
    </div>
  );
}
