// orbFeed.ts — shared render source for the capsule's thinking orb.
//
// Requirement: every assistant message avatar shows the rotating capsule thinking
// animation (SiriGL orb, 6 white metaball dots weaving). One SiriGL per avatar means
// one WebGL context per avatar — the browser caps ~16, so long chats would blow up.
//
// Approach: module-level singleton. The first subscriber creates an offscreen canvas
// with the single GL context running the orb loop (shader identical to SiriGL,
// speed=1.5 matching capsule thinking). After each frame it synchronously notifies
// all subscribers, who mirror the source via 2D drawImage onto their own small
// canvas (same-task copy, no preserveDrawingBuffer needed). When the last subscriber
// leaves, stop the loop and release GL resources. With the window hidden, rAF is
// suspended by WebKit/Chromium and the loop naturally pauses.

import { ORB_FRAGMENT_SRC, VERTEX_SRC } from '../SiriGL';

type FrameSubscriber = (source: HTMLCanvasElement) => void;

/** Source resolution: avatars are at most ~40px@2x; 96 is plenty sharp at negligible render cost. */
const SOURCE_SIZE = 96;
/** Rotation speed matching the capsule thinking state (Capsule.tsx: transcribing/polishing → speed 1.5). */
const SPEED = 1.5;

interface Feed {
  canvas: HTMLCanvasElement;
  dispose: () => void;
}

const subscribers = new Set<FrameSubscriber>();
let feed: Feed | null = null;

function startFeed(): Feed | null {
  const canvas = document.createElement('canvas');
  canvas.width = SOURCE_SIZE;
  canvas.height = SOURCE_SIZE;
  const gl = canvas.getContext('webgl', {
    alpha: true,
    premultipliedAlpha: true,
    antialias: false,
  });
  if (!gl) return null;

  const compile = (type: number, src: string): WebGLShader | null => {
    const shader = gl.createShader(type);
    if (!shader) return null;
    gl.shaderSource(shader, src);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      console.error('[orbFeed] shader compile failed:', gl.getShaderInfoLog(shader));
      gl.deleteShader(shader);
      return null;
    }
    return shader;
  };

  const vs = compile(gl.VERTEX_SHADER, VERTEX_SRC);
  const fs = compile(gl.FRAGMENT_SHADER, ORB_FRAGMENT_SRC);
  if (!vs || !fs) return null;
  const program = gl.createProgram();
  if (!program) return null;
  gl.attachShader(program, vs);
  gl.attachShader(program, fs);
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    console.error('[orbFeed] program link failed:', gl.getProgramInfoLog(program));
    return null;
  }
  gl.useProgram(program);

  const buffer = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  const aPos = gl.getAttribLocation(program, 'aPos');
  gl.enableVertexAttribArray(aPos);
  gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 0, 0);

  const uResolution = gl.getUniformLocation(program, 'iResolution');
  const uTime = gl.getUniformLocation(program, 'iTime');
  const uGather = gl.getUniformLocation(program, 'uGather');
  gl.viewport(0, 0, SOURCE_SIZE, SOURCE_SIZE);

  let shaderTime = 0;
  let raf = 0;
  let last = performance.now();

  const frame = (now: number) => {
    const dt = Math.min(0.05, (now - last) / 1000);
    last = now;
    shaderTime += dt * SPEED;
    gl.uniform2f(uResolution, SOURCE_SIZE, SOURCE_SIZE);
    gl.uniform1f(uTime, shaderTime);
    // Avatar form stays spread out and rotating (no entrance gather: mirror targets
    // mount/unmount anytime).
    if (uGather) gl.uniform1f(uGather, 0);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    // Notify subscribers to drawImage within the same task — the WebGL frame stays
    // valid until this task ends.
    for (const cb of subscribers) cb(canvas);
    raf = requestAnimationFrame(frame);
  };
  raf = requestAnimationFrame(frame);

  return {
    canvas,
    dispose() {
      cancelAnimationFrame(raf);
      gl.deleteBuffer(buffer);
      gl.deleteProgram(program);
      gl.deleteShader(vs);
      gl.deleteShader(fs);
      gl.getExtension('WEBGL_lose_context')?.loseContext();
    },
  };
}

/**
 * Subscribe to the shared orb: called after each frame (arg is the source canvas;
 * copy synchronously via drawImage). Returns an unsubscribe function, or null when
 * GL is unavailable (caller degrades on its own).
 */
export function subscribeOrbFrames(cb: FrameSubscriber): (() => void) | null {
  if (!feed) {
    feed = startFeed();
    if (!feed) return null;
  }
  subscribers.add(cb);
  return () => {
    subscribers.delete(cb);
    if (subscribers.size === 0 && feed) {
      feed.dispose();
      feed = null;
    }
  };
}
