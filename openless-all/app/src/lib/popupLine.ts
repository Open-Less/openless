/** Pull a short tail back onto the previous line. A wrapped line of one or two
 *  characters is harder to read than a slightly wider popup. */
export function avoidShortLastLine(node: HTMLElement, minLast = 4): void {
  const text = node.textContent ?? '';
  const total = [...text].length;
  if (total <= minLast) return;
  const cap = Math.min(360, Math.max(node.getBoundingClientRect().width, 1) + 160);
  for (let attempt = 0; attempt < 24; attempt += 1) {
    const last = lastLineLength(node);
    if (last >= minLast || last >= total) return;
    const next = Math.ceil(node.getBoundingClientRect().width + 14);
    if (next > cap) return;
    node.style.width = `${next}px`;
    node.style.maxWidth = `${next}px`;
  }
}

function lastLineLength(node: HTMLElement): number {
  const textNode = firstText(node);
  if (!textNode) return 0;
  const text = textNode.textContent ?? '';
  if (!text) return 0;
  const range = document.createRange();
  let lineStart = 0;
  let lineTop = 0;
  for (let index = 0; index < text.length; index += 1) {
    range.setStart(textNode, index);
    range.setEnd(textNode, index + 1);
    const top = range.getBoundingClientRect().top;
    if (index === 0) lineTop = top;
    if (top > lineTop + 1) {
      lineStart = index;
      lineTop = top;
    }
  }
  return [...text.slice(lineStart)].length;
}

function firstText(node: Node): Text | null {
  if (node.nodeType === Node.TEXT_NODE) return node as Text;
  for (const child of node.childNodes) {
    const found = firstText(child);
    if (found) return found;
  }
  return null;
}
