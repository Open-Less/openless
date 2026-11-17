import { collectCapsuleInputRegions } from './capsuleInputRegions';
class Node {
  constructor(
    readonly role: string,
    readonly rect: number[],
    readonly parentElement: Node | null = null,
    readonly hidden = false,
  ) {}
  closest(selector: string): Node | null {
    if (selector.includes('aria-hidden') && this.hidden) return this;
    if (selector.includes('listbox') && (this.role === 'listbox' || this.role === 'menu'))
      return this;
    return this.parentElement?.closest(selector) ?? null;
  }
  matches() {
    return false;
  }
  getClientRects() {
    return [this.getBoundingClientRect()];
  }
  getBoundingClientRect() {
    const [left, top, width, height] = this.rect;
    return { left, top, right: left + width, bottom: top + height, width, height };
  }
}
const list = new Node('listbox', [100, 20, 180, 100]);
const button = new Node('button', [10, 130, 24, 24]);
const nodes = [
  button,
  new Node('button', [50, 50, 1, 1]),
  list,
  ...Array.from({ length: 300 }, (_, i) => new Node('button', [100, 20 + i * 24, 180, 24], list)),
  new Node('button', [900, 0, 20, 20]),
  new Node('button', [20, 20, 24, 24], null, true),
];
const doc = {
  defaultView: {
    innerWidth: 460,
    innerHeight: 172,
    getComputedStyle: () => ({ visibility: 'visible' }),
  },
  querySelectorAll: (selector: string) =>
    nodes.filter((n) => n.role === 'button' || selector.includes(`[role="${n.role}"]`)),
} as unknown as Document;
const result = collectCapsuleInputRegions(doc);
if (
  JSON.stringify(result) !==
  JSON.stringify([
    { left: 10, top: 130, width: 24, height: 24 },
    { left: 100, top: 20, width: 180, height: 100 },
  ])
)
  throw new Error('only visible buttons and the menu viewport may own input');
console.log('capsule menu hit ownership, hidden controls and viewport clipping passed');
