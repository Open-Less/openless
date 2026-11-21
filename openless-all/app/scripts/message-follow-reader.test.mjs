import assert from 'node:assert/strict';
import { installMessageFollow } from '../src/lib/messageFollow.ts';

class Viewport extends EventTarget {
  scrollHeight = 1000;
  clientHeight = 100;
  clientLeft = 0;
  clientWidth = 100;
  getBoundingClientRect() {
    return { left: 0 };
  }
  position = 900;
  get scrollTop() {
    return this.position;
  }
  set scrollTop(value) {
    this.position = Math.max(0, Math.min(value, this.scrollHeight - this.clientHeight));
  }
}
globalThis.window = new EventTarget();
const viewport = new Viewport();
const content = new EventTarget();
const root = new EventTarget();
root.querySelector = (selector) => (selector.includes('viewport') ? viewport : content);
let resize,
  disconnected = false,
  nextFrame = 0;
const frames = new Map();
globalThis.ResizeObserver = class {
  constructor(callback) {
    resize = callback;
  }
  observe() {}
  disconnect() {
    disconnected = true;
  }
};
globalThis.requestAnimationFrame = (callback) => {
  frames.set(++nextFrame, callback);
  return nextFrame;
};
globalThis.cancelAnimationFrame = (id) => frames.delete(id);
const flush = () => {
  for (const [id, callback] of frames) {
    frames.delete(id);
    callback();
  }
};
const dispose = installMessageFollow(root);
resize();
flush();
assert.equal(viewport.scrollTop, 900);
// Resize deliveries can precede the animation frame that moves to the new bottom.
viewport.scrollHeight = 1080;
resize();
viewport.scrollHeight = 1160;
resize();
flush();
assert.equal(viewport.scrollTop, 1060, 'geometry changes do not revoke bottom ownership');
for (const [type, clientY] of [
  ['touchstart', 20],
  ['touchmove', 50],
]) {
  const event = new Event(type);
  Object.defineProperty(event, 'touches', { value: [{ clientY }] });
  viewport.dispatchEvent(event);
}
viewport.scrollTop = 800;
viewport.dispatchEvent(new Event('scroll'));
// Browser scroll anchoring and a delayed ResizeObserver can put scrollTop beyond
// the previous content height while the reader remains above the current bottom.
viewport.scrollHeight = 1300;
viewport.scrollTop = 1000;
resize();
flush();
assert.equal(viewport.scrollTop, 1000, 'new content must not reclaim scrolling from the reader');
viewport.scrollTop = 1200;
viewport.dispatchEvent(new Event('scroll'));
viewport.scrollHeight = 1380;
resize();
flush();
assert.equal(viewport.scrollTop, 1280, 'returning to the actual bottom resumes follow');
// A large upward wheel gesture can already have reached the top at delivery.
viewport.scrollTop = 0;
const wheel = new Event('wheel');
Object.defineProperty(wheel, 'deltaY', { value: -1400 });
viewport.dispatchEvent(wheel);
viewport.scrollHeight = 1400;
resize();
flush();
assert.equal(viewport.scrollTop, 0, 'streaming must not interrupt a reader at the top');
viewport.scrollTop = 1300;
viewport.dispatchEvent(new Event('scroll'));

const down = new Event('pointerdown');
Object.defineProperties(down, {
  button: { value: 0 },
  pointerType: { value: 'mouse' },
  clientX: { value: 103 },
});
viewport.dispatchEvent(down);
viewport.scrollTop = 900;
viewport.dispatchEvent(new Event('scroll'));
window.dispatchEvent(new Event('pointerup'));
viewport.scrollHeight = 1500;
resize();
flush();
assert.equal(viewport.scrollTop, 900, 'dragging the scrollbar hands follow to the reader');
dispose();
assert(disconnected);
assert.equal(frames.size, 0);
console.log('Message follow preserves reader ownership through content growth and anchoring');
