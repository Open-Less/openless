// orbFeed.ts — 共享的胶囊思考 orb 渲染源。
//
// 需求：每条助手消息的头像都要显示**旋转中的**胶囊思考动画（SiriGL orb，
// 6 白色 metaball 圆点交错转动）。若每个头像各挂一个 SiriGL，就是每头像一个
// WebGL context —— 浏览器上限 ~16 个，长对话必炸。
//
// 方案：模块级单例。首个订阅者到来时创建一块**离屏 canvas + 唯一 GL context**
// 跑 orb 渲染循环（shader 与 SiriGL 完全同源，speed=1.5 与胶囊思考一致）；
// 每帧渲染完成后**同步**回调所有订阅者，订阅者用 2D drawImage 把源画面镜像到
// 自己的小 canvas（同一任务内拷贝，无需 preserveDrawingBuffer）。最后一个订阅者
// 离开时停表并释放 GL 资源。窗口隐藏时 rAF 被 WebKit/Chromium 挂起，循环自然停。

import { ORB_FRAGMENT_SRC, VERTEX_SRC } from '../SiriGL';

type FrameSubscriber = (source: HTMLCanvasElement) => void;

/** 源分辨率：头像最大 ~40px@2x，96 足够清晰且渲染开销可忽略。 */
const SOURCE_SIZE = 96;
/** 与胶囊思考态一致的转速（Capsule.tsx: transcribing/polishing → speed 1.5）。 */
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
    // 头像形态常驻散开转动（不做出场聚拢：镜像目标随时挂载/卸载）。
    if (uGather) gl.uniform1f(uGather, 0);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    // 同一任务内通知订阅者 drawImage —— WebGL 画面在本任务结束前有效。
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
 * 订阅共享 orb：每帧渲染后回调（参数是源 canvas，同步 drawImage 拷贝）。
 * 返回退订函数。GL 不可用时返回 null（调用方自行降级）。
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
