function assertEqual(actual: unknown, expected: unknown, message = 'values differ') {
  if (actual !== expected) throw new Error(`${message}: ${String(actual)} !== ${String(expected)}`);
}
function assertOk(value: unknown, message = 'expected a truthy value'): asserts value {
  if (!value) throw new Error(message);
}
import { subscribeOrbFrames } from './orbFeed';
import { isWebGLAvailable } from '../SiriGL';

type Failure = 'shader' | 'program' | 'link' | 'buffer' | 'none';
let failure: Failure = 'none';
let lost = 0;
let deletedShaders = 0;
let deletedPrograms = 0;
let deletedBuffers = 0;
let contexts = 0;
const frames = new Map<number, FrameRequestCallback>();
const listeners = new Map<string, EventListener>();
let nextFrame = 0;
const gl = {
  VERTEX_SHADER: 1,
  FRAGMENT_SHADER: 2,
  COMPILE_STATUS: 3,
  LINK_STATUS: 4,
  ARRAY_BUFFER: 5,
  STATIC_DRAW: 6,
  FLOAT: 7,
  TRIANGLES: 8,
  createShader: () => ({}),
  shaderSource() {},
  compileShader() {},
  getShaderParameter: () => failure !== 'shader',
  getShaderInfoLog: () => 'expected fixture failure',
  deleteShader: () => deletedShaders++,
  createProgram: () => (failure === 'program' ? null : {}),
  attachShader() {},
  linkProgram() {},
  getProgramParameter: () => failure !== 'link',
  getProgramInfoLog: () => 'expected fixture failure',
  deleteProgram: () => deletedPrograms++,
  useProgram() {},
  createBuffer: () => (failure === 'buffer' ? null : {}),
  bindBuffer() {},
  bufferData() {},
  getAttribLocation: () => 0,
  enableVertexAttribArray() {},
  vertexAttribPointer() {},
  getUniformLocation: () => null,
  viewport() {},
  uniform2f() {},
  uniform1f() {},
  drawArrays() {},
  deleteBuffer: () => deletedBuffers++,
  getExtension: () => ({ loseContext: () => lost++ }),
};
const original = Object.getOwnPropertyDescriptor(globalThis, 'document');
const originalRaf = globalThis.requestAnimationFrame;
const originalCancel = globalThis.cancelAnimationFrame;
const error = console.error;
const doc = {
  hidden: false,
  createElement: () => ({
    width: 0,
    height: 0,
    getContext: () => {
      contexts++;
      return gl;
    },
  }),
  addEventListener: (name: string, callback: EventListener) => listeners.set(name, callback),
  removeEventListener: (name: string) => listeners.delete(name),
};
Object.defineProperty(globalThis, 'document', { value: doc, configurable: true });
globalThis.requestAnimationFrame = (callback) => {
  const id = ++nextFrame;
  frames.set(id, callback);
  return id;
};
globalThis.cancelAnimationFrame = (id) => {
  frames.delete(id);
};
console.error = () => {};
try {
  assertEqual(isWebGLAvailable(), true);
  assertEqual(lost, 1, 'the capability probe releases its temporary GPU context');
  assertEqual(isWebGLAvailable(), true);
  assertEqual(contexts, 1, 'availability remains a one-shot probe');
  for (const stage of ['shader', 'program', 'link', 'buffer'] as const) {
    failure = stage;
    const before = lost;
    assertEqual(
      subscribeOrbFrames(() => {}),
      null,
      stage,
    );
    assertEqual(lost, before + 1, `${stage}: failed initialization releases its context`);
    assertEqual(frames.size, 0, `${stage}: no orphan animation loop`);
  }
  failure = 'none';
  const before = lost;
  const stop = subscribeOrbFrames(() => {});
  assertOk(stop);
  assertEqual(frames.size, 1);
  doc.hidden = true;
  listeners.get('visibilitychange')!(new Event('visibilitychange'));
  assertEqual(frames.size, 0, 'hidden windows stop drawing');
  doc.hidden = false;
  listeners.get('visibilitychange')!(new Event('visibilitychange'));
  assertEqual(frames.size, 1, 'resume creates exactly one RAF chain');
  stop();
  stop();
  assertEqual(lost, before + 1, 'last unsubscribe disposes exactly once');
  assertEqual(frames.size, 0);
  assertEqual(listeners.size, 0);
  assertOk(deletedShaders >= 2 && deletedPrograms >= 1 && deletedBuffers >= 1);
  let selfStop: (() => void) | null = null;
  selfStop = subscribeOrbFrames(() => selfStop?.());
  const [id, callback] = [...frames.entries()][0];
  frames.delete(id);
  callback(performance.now());
  assertEqual(frames.size, 0, 'unsubscribe during a frame cannot resurrect the disposed loop');
  console.log('WebGL probe, failure cleanup, visibility and final subscription lifecycle passed');
} finally {
  console.error = error;
  globalThis.requestAnimationFrame = originalRaf;
  globalThis.cancelAnimationFrame = originalCancel;
  if (original) Object.defineProperty(globalThis, 'document', original);
  else Reflect.deleteProperty(globalThis, 'document');
}
