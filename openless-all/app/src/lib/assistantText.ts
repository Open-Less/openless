import { segmentInsertUnits, selectInsertTextWindow } from './insertTextAnimation';

const CJK = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u;
const WORD = /[\p{L}\p{N}\p{M}]/u;
const EMOJI = /[\p{Extended_Pictographic}\p{Regional_Indicator}]/u;

function canSegmentGraphemes(): boolean {
  return typeof (Intl as typeof Intl & { Segmenter?: unknown }).Segmenter === 'function';
}

/** Last two words (Latin) or characters (CJK), including their trailing punctuation. */
export function proseTailStart(text: string): number | null {
  // Old webviews may only split code points. Prefer native wrapping over breaking a cluster.
  if (
    !canSegmentGraphemes() &&
    /[\p{M}\p{Extended_Pictographic}\p{Regional_Indicator}\u200d]/u.test(text)
  )
    return null;
  const starts: number[] = [];
  let offset = 0;
  let inWord = false;
  for (const glyph of segmentInsertUnits(text)) {
    if (CJK.test(glyph) || EMOJI.test(glyph)) {
      starts.push(offset);
      inWord = false;
    } else if (WORD.test(glyph)) {
      if (!inWord) starts.push(offset);
      inWord = true;
    } else if (!/['’\-]/u.test(glyph)) {
      inWord = false;
    }
    offset += glyph.length;
  }
  return starts.length >= 2 ? starts[starts.length - 2] : null;
}

/**
 * Group only the prose tail, not the whole paragraph. Unlike inserted NBSP/word-joiners,
 * this leaves copied text intact. Range clones preserve emphasis and link attributes.
 * Explicit line breaks, code blocks and nested block structure remain authoritative.
 */
export function keepProseTailsTogether(root: HTMLElement): void {
  for (const block of root.querySelectorAll<HTMLElement>('p, li')) {
    if (block.querySelector('p, ul, ol, pre, table')) continue;
    const walker = document.createTreeWalker(block, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT);
    let nodes: Text[] = [];
    while (walker.nextNode()) {
      const node = walker.currentNode;
      if (node.nodeName === 'BR' || node.nodeName === 'IMG') nodes = [];
      else if (node.nodeType === Node.TEXT_NODE) nodes.push(node as Text);
    }
    const start = proseTailStart(nodes.map((node) => node.data).join(''));
    if (start === null) continue;
    let remaining = start;
    const first = nodes.find((node) => {
      if (remaining < node.length) return true;
      remaining -= node.length;
      return false;
    });
    if (!first) continue;

    const tail = document.createRange();
    tail.selectNodeContents(block);
    // Never split a link into two focus targets, or break up an inline code token.
    const atomic = first.parentElement?.closest('a, code');
    if (atomic && block.contains(atomic)) tail.setStartBefore(atomic);
    else tail.setStart(first, remaining);
    const before = document.createRange();
    before.selectNodeContents(block);
    before.setEnd(tail.startContainer, tail.startOffset);
    const wrapper = document.createElement('span');
    wrapper.className = 'ol-prose-tail';
    wrapper.append(tail.cloneContents());
    block.replaceChildren(before.cloneContents(), wrapper);
  }
}

export const ANSWER_REVEAL = {
  durationMs: 90,
  staggerMs: 8,
  maxLeadMs: 90,
  maxGlyphs: 96,
} as const;

export interface RevealGlyph {
  text: string;
  offset: number;
  start: number;
}
export interface TextRevealState {
  text: string;
  glyphs: RevealGlyph[];
}

/**
 * Absolute birth times survive innerHTML reparses: already-visible text never restarts.
 * Markdown rewrites (e.g. closing ** or a link) settle immediately rather than flashing
 * the old paragraph. Only an appended tail is animated; bursts have a bounded catch-up.
 */
export function planTextReveal(
  previous: TextRevealState,
  text: string,
  now: number,
  animate: boolean,
): TextRevealState {
  if (!animate || !canSegmentGraphemes()) return { text, glyphs: [] };
  const prefix = previous.text.trimEnd();
  if (!text.startsWith(prefix)) return { text, glyphs: [] };

  const glyphs = previous.glyphs.filter(
    (glyph) =>
      glyph.offset + glyph.text.length <= prefix.length &&
      glyph.start + ANSWER_REVEAL.durationMs > now,
  );
  const delta = text.slice(prefix.length);
  const window = selectInsertTextWindow(delta, {
    maxUnits: ANSWER_REVEAL.maxGlyphs,
    overflowBufferUnits: 0,
  });
  let offset = text.length - window.length;
  const added: { text: string; offset: number }[] = [];
  for (const glyph of segmentInsertUnits(window)) {
    if (/\S/u.test(glyph)) added.push({ text: glyph, offset });
    offset += glyph.length;
  }
  const start = Math.max(now, glyphs[glyphs.length - 1]?.start ?? now);
  const stagger = Math.min(
    ANSWER_REVEAL.staggerMs,
    Math.max(0, now + ANSWER_REVEAL.maxLeadMs - start) / Math.max(1, added.length - 1),
  );
  added.forEach((glyph, index) => glyphs.push({ ...glyph, start: start + index * stagger }));
  return { text, glyphs: glyphs.slice(-ANSWER_REVEAL.maxGlyphs) };
}

/** Temporary inline spans preserve wrapping/selection; no per-character React state or RAF. */
export function mountTextReveals(
  root: HTMLElement,
  glyphs: RevealGlyph[],
  now: number,
): () => void {
  const animations: Animation[] = [];
  if (!glyphs.length || typeof root.animate !== 'function') return () => {};
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const nodes: Text[] = [];
  while (walker.nextNode()) nodes.push(walker.currentNode as Text);
  let offset = 0;
  for (const node of nodes) {
    const end = offset + node.length;
    const active = glyphs.filter(
      (glyph) => glyph.offset >= offset && glyph.offset + glyph.text.length <= end,
    );
    if (active.length && !node.parentElement?.closest('pre, code')) {
      const fragment = document.createDocumentFragment();
      let cursor = 0;
      for (const glyph of active) {
        const index = glyph.offset - offset;
        fragment.append(node.data.slice(cursor, index));
        const span = document.createElement('span');
        span.className = 'olchat-reveal-glyph';
        span.textContent = glyph.text;
        fragment.append(span);
        cursor = index + glyph.text.length;
      }
      fragment.append(node.data.slice(cursor));
      const spans = Array.from(fragment.querySelectorAll<HTMLElement>('.olchat-reveal-glyph'));
      node.replaceWith(fragment);
      spans.forEach((span, index) => {
        const animation = span.animate(
          [
            { opacity: 0, filter: 'blur(2px)' },
            { opacity: 1, filter: 'blur(0px)' },
          ],
          { duration: ANSWER_REVEAL.durationMs, easing: 'ease-out', fill: 'both' },
        );
        animation.id = 'ol-answer-reveal';
        animation.currentTime = now - active[index].start;
        animation.onfinish = () => {
          span.replaceWith(document.createTextNode(span.textContent ?? ''));
          // Release the filter layer as soon as the character is sharp (WKWebView).
          animation.cancel();
        };
        animations.push(animation);
      });
    }
    offset = end;
  }
  return () =>
    animations.forEach((animation) => {
      animation.onfinish = null;
      animation.cancel();
    });
}
