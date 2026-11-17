/** Pull a short tail back onto the previous line. A wrapped line of one or two
 *  characters is harder to read than a slightly wider popup. */
export function avoidShortLastLine(node: HTMLElement, minLast = 4): void {
  const text = node.textContent ?? '';
  const total = [...text].length;
  if (total <= minLast) return;
  // offsetWidth is in the same design pixels as the inline width, even in a zoomed popup.
  const cap = 360;
  for (let attempt = 0; attempt < 24; attempt += 1) {
    const last = lastLineLength(node);
    if (last >= minLast || last >= total) return;
    const current = node.offsetWidth;
    const next = current + 14;
    if (next > cap) return;
    node.style.width = `${next}px`;
    if (node.offsetWidth <= current) {
      node.style.width = `${current}px`;
      return;
    }
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
  let index = 0;
  for (const char of text) {
    range.setStart(textNode, index);
    range.setEnd(textNode, index + char.length);
    const top = range.getBoundingClientRect().top;
    if (index === 0) lineTop = top;
    if (top > lineTop + 1) {
      lineStart = index;
      lineTop = top;
    }
    index += char.length;
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
