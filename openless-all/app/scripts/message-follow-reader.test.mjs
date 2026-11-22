import assert from 'node:assert/strict';
import { installMessageFollow } from '../src/lib/messageFollow.ts';

class Viewport extends EventTarget {
  scrollHeight = 1000;
  clientHeight = 100;
  position = 900;
  get scrollTop() {
    return this.position;
  }
  set scrollTop(value) {
    this.position = Math.max(0, Math.min(value, this.scrollHeight - this.clientHeight));
  }
}
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
viewport.dispatchEvent(new Event('touchmove'));
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
dispose();
assert(disconnected);
assert.equal(frames.size, 0);
console.log('Message follow preserves reader ownership through content growth and anchoring');
