import { marked } from 'marked';
export interface QaSource {
  url: string;
  host: string;
  title: string;
}
export function qaSources(markdown: string): { body: string; sources: QaSource[] } {
  const sources: QaSource[] = [];
  marked.walkTokens(marked.lexer(markdown), (token) => {
    if (token.type !== 'link') return;
    try {
      const url = new URL(token.href);
      if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) return;
      if (sources.some((source) => source.url === url.href)) return;
      sources.push({
        url: url.href,
        host: url.hostname.replace(/^www\./, ''),
        title: /^\d+$/.test(token.text) ? url.hostname : token.text,
      });
    } catch {
      /* Invalid citation URLs are never interactive. */
    }
  });
  // Native search emits a numeric citation block after its answer text.
  const body = markdown.replace(/\n\n(?:\[\d+\]\(<https?:\/\/[^>]+>\)\s*)+$/, '').trimEnd();
  return { body, sources: sources.slice(0, 12) };
}
