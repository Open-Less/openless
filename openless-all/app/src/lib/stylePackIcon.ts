import DOMPurify from 'dompurify';

const MAX_SVG_BYTES = 256 * 1024;
const MAX_PNG_BYTES = 64 * 1024;
const ICON_SIZE = 128;

/** 清理 SVG 的可执行内容和外部引用，保留图形、渐变与内部遮罩。 */
export function sanitizeStyleSvg(source: string): string {
  if (/<!DOCTYPE|<!ENTITY/i.test(source)) throw new Error('invalidSvg');
  const document = new DOMParser().parseFromString(source, 'image/svg+xml');
  if (document.querySelector('parsererror') || document.documentElement.localName !== 'svg') {
    throw new Error('invalidSvg');
  }
  const clean = DOMPurify.sanitize(source, {
    USE_PROFILES: { svg: true, svgFilters: true },
    FORBID_TAGS: [
      'script',
      'foreignObject',
      'image',
      'feImage',
      'iframe',
      'object',
      'embed',
      'animate',
      'animateMotion',
      'animateTransform',
      'set',
    ],
  });
  const svg = new DOMParser().parseFromString(clean, 'image/svg+xml').documentElement;
  if (svg.localName !== 'svg') throw new Error('invalidSvg');
  // 只允许引用同一 SVG 中的元素，避免解码图标时请求网络或本地文件。
  for (const element of [svg, ...Array.from(svg.querySelectorAll('*'))]) {
    for (const attribute of Array.from(element.attributes)) {
      if (attribute.localName === 'href' && !attribute.value.trim().startsWith('#')) {
        throw new Error('invalidSvg');
      }
      if (hasExternalCssReference(attribute.value)) throw new Error('invalidSvg');
    }
    if (element.localName === 'style' && hasExternalCssReference(element.textContent ?? '')) {
      throw new Error('invalidSvg');
    }
  }
  if (!svg.hasAttribute('viewBox')) {
    const width = Number.parseFloat(svg.getAttribute('width') ?? '24');
    const height = Number.parseFloat(svg.getAttribute('height') ?? '24');
    if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0)
      throw new Error('invalidSvg');
    svg.setAttribute('viewBox', `0 0 ${width} ${height}`);
  }
  svg.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
  svg.setAttribute('width', String(ICON_SIZE));
  svg.setAttribute('height', String(ICON_SIZE));
  svg.setAttribute('preserveAspectRatio', 'xMidYMid meet');
  return new XMLSerializer().serializeToString(svg);
}

function hasExternalCssReference(value: string): boolean {
  return (
    /@import/i.test(value) ||
    Array.from(value.matchAll(/url\s*\(\s*(['"]?)(.*?)\1\s*\)/gi)).some(
      (match) => !match[2].trim().startsWith('#'),
    )
  );
}

/** 转成风格包已有的 PNG 资源格式；128px 输出仍须满足 Core 的 64 KiB 上限。 */
export async function rasterizeStyleSvg(file: File): Promise<number[]> {
  if (
    !file.name.toLowerCase().endsWith('.svg') ||
    (file.type && file.type !== 'image/svg+xml') ||
    file.size > MAX_SVG_BYTES
  ) {
    throw new Error('invalidSvg');
  }
  const svg = sanitizeStyleSvg(await file.text());
  const url = URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml' }));
  try {
    const image = new Image();
    image.src = url;
    await image.decode();
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = ICON_SIZE;
    const context = canvas.getContext('2d');
    if (!context) throw new Error('invalidSvg');
    context.drawImage(image, 0, 0, ICON_SIZE, ICON_SIZE);
    const encoded = canvas.toDataURL('image/png').split(',')[1];
    const bytes = Array.from(atob(encoded), (character) => character.charCodeAt(0));
    if (bytes.length > MAX_PNG_BYTES) throw new Error('invalidSvg');
    return bytes;
  } finally {
    URL.revokeObjectURL(url);
  }
}

/** 图片仅通过 img 展示；拒绝可执行类型和超过资源上限的返回值。 */
export function isStyleIconDataUrl(value: string | null): value is string {
  return (
    value !== null &&
    value.length < 90_000 &&
    /^data:image\/(?:png|jpeg|webp);base64,[A-Za-z0-9+/]+={0,2}$/.test(value)
  );
}
