const widths = new Map<string, number>();
let measuringText: SVGTextElement | null = null;

/** Canvas 2D has no numeric-feature switch in WebKit. SVG supplies shaped advances
 * once per font/digit, independent of document zoom and CSS letter spacing. */
export function measureTabularDigit(digit: string, font: string): number | null {
  if (!/^[0-9]$/.test(digit) || typeof document === 'undefined') return null;
  const key = `${font}\0${digit}`;
  const cached = widths.get(key);
  if (cached !== undefined) return cached;
  if (!measuringText) {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('width', '0');
    svg.setAttribute('height', '0');
    svg.setAttribute('aria-hidden', 'true');
    svg.style.cssText =
      'position:absolute;width:0;height:0;overflow:hidden;visibility:hidden;pointer-events:none';
    measuringText = document.createElementNS('http://www.w3.org/2000/svg', 'text');
    svg.appendChild(measuringText);
    (document.body ?? document.documentElement).appendChild(svg);
    document.fonts?.addEventListener('loadingdone', () => widths.clear());
  }
  measuringText.style.font = font;
  measuringText.style.fontVariantNumeric = 'tabular-nums';
  measuringText.style.letterSpacing = '0px';
  measuringText.style.wordSpacing = '0px';
  measuringText.textContent = digit;
  const width = measuringText.getComputedTextLength();
  if (!Number.isFinite(width) || width <= 0) return null;
  if (widths.size >= 128) widths.clear();
  widths.set(key, width);
  return width;
}
