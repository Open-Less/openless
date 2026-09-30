// Less Computer fullscreen rainbow edge glow (standalone window=less-computer-glow).
// Draws only edge-hugging light bands, no dark overlay; the colored halo flows along the
// edges, mimicking Apple Intelligence's glowing outline.
// Purely visual: pointer-events:none, plus the backend's set_ignore_cursor_events(true).
// macOS only.
//
// v2: a single-pass WebGL fragment shader replaces the old "4-layer conic-gradient +
// CSS blur" — the old approach relied on @property angle animations that re-rasterized 4
// gradient layers every frame plus a blur each (fullscreen 4x blur, expensive, and a 15px
// blur couldn't produce a wide halo). The shader computes a rounded-rect SDF + double
// exponential distance falloff in one pass for "sharp edge line + inner glow + wide-tail
// outer glow" (true Gaussian-like feel), keeping the EdgeGlow iridescent 12-color neon
// spectrum (purple→blue→cyan→green→gold→orange→red→pink), flowing along the edge over 8s
// with a slow breathe.
// The halo is a low-frequency signal, so it renders internally at half resolution
// (RENDER_SCALE); even fullscreen 5K is just one lightweight draw.

import { useEffect, useRef, useState } from 'react';

const RENDER_SCALE = 0.5;
/** Matches the old CSS --lcg-radius default: hugs the screen's physical rounded corners. */
const SCREEN_CORNER_RADIUS = 42;

const VERTEX_SRC = 'attribute vec2 aPos; void main(){ gl_Position=vec4(aPos,0.0,1.0); }';

// EdgeGlow iridescent theme's 12 color stops (same as the old conic-gradient), interpolated
// linearly in the shader.
const GLOW_FRAGMENT_SRC = `
precision highp float;
uniform vec2 iResolution; uniform float iTime;
uniform float uRadius;
const float TAU = 6.28318530718;

vec3 spectrum(float h) {
  vec3 c0 = vec3(0.651, 0.200, 0.949);  /* #a633f2 */
  vec3 c1 = vec3(0.349, 0.302, 1.000);  /* #594dff */
  vec3 c2 = vec3(0.149, 0.502, 1.000);  /* #2680ff */
  vec3 c3 = vec3(0.051, 0.702, 0.980);  /* #0db3fa */
  vec3 c4 = vec3(0.102, 0.851, 0.902);  /* #1ad9e6 */
  vec3 c5 = vec3(0.200, 0.902, 0.749);  /* #33e6bf */
  vec3 c6 = vec3(0.502, 0.851, 0.400);  /* #80d966 */
  vec3 c7 = vec3(0.800, 0.702, 0.102);  /* #ccb31a */
  vec3 c8 = vec3(1.000, 0.502, 0.102);  /* #ff801a */
  vec3 c9 = vec3(1.000, 0.251, 0.349);  /* #ff4059 */
  vec3 ca = vec3(0.949, 0.149, 0.600);  /* #f22699 */
  vec3 cb = vec3(0.749, 0.251, 0.851);  /* #bf40d9 */
  float x = fract(h) * 12.0;
  float i = floor(x);
  float f = x - i;
  vec3 a = c0; vec3 b = c1;
  if (i < 1.0)      { a = c0; b = c1; }
  else if (i < 2.0) { a = c1; b = c2; }
  else if (i < 3.0) { a = c2; b = c3; }
  else if (i < 4.0) { a = c3; b = c4; }
  else if (i < 5.0) { a = c4; b = c5; }
  else if (i < 6.0) { a = c5; b = c6; }
  else if (i < 7.0) { a = c6; b = c7; }
  else if (i < 8.0) { a = c7; b = c8; }
  else if (i < 9.0) { a = c8; b = c9; }
  else if (i < 10.0){ a = c9; b = ca; }
  else if (i < 11.0){ a = ca; b = cb; }
  else              { a = cb; b = c0; }
  return mix(a, b, f);
}

/* Rounded-rect SDF (positive outside). */
float sdRoundRect(vec2 p, vec2 half_, float r) {
  vec2 q = abs(p) - (half_ - vec2(r));
  return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

void main() {
  vec2 R = iResolution.xy;
  vec2 p = gl_FragCoord.xy - R * 0.5;
  /* Inward distance to the screen edge (px, 0 at the edge growing inward). */
  float d = max(-sdRoundRect(p, R * 0.5, uRadius), 0.0);

  float t = iTime;
  /* Hue = angular position along the edge + flow of 8s/revolution (same speed as the old conic spin). */
  float hue = fract(atan(p.y, p.x) / TAU + t / 8.0);
  vec3 col = spectrum(hue);
  /* Neon gain matching the old per-layer saturate(1.2~1.3)·brightness(1.05). */
  col = clamp((col - 0.5) * 1.25 + 0.5, 0.0, 1.0) * 1.06;

  /* Glow profile: sharp edge line + inner glow + wide-tail outer glow (double exponential ≈ Gaussian tail). */
  float line  = smoothstep(3.5, 0.6, d) * 1.05;
  float inner = exp(-d / 15.0) * 0.60;
  float outer = exp(-d / 52.0) * 0.34;
  /* Slow breathe (merged replacement for the old lcg-breathe). */
  float breathe = 1.0 + 0.12 * sin(t * 1.15) * sin(t * 0.53 + 1.7);
  float glow = (line + inner + outer) * breathe;

  vec3 rgb = col * glow;
  float a = clamp(max(rgb.r, max(rgb.g, rgb.b)), 0.0, 1.0);
  gl_FragColor = vec4(rgb, a);
}`;

const glowCss = `
html, body, #root { background: transparent !important; margin: 0; height: 100%; overflow: hidden; }
.lcg-root {
  position: fixed;
  inset: 0;
  pointer-events: none;
}
.lcg-root canvas { display: block; width: 100%; height: 100%; }
`;

if (typeof document !== 'undefined' && !document.getElementById('less-computer-glow-style')) {
  const tag = document.createElement('style');
  tag.id = 'less-computer-glow-style';
  tag.textContent = glowCss;
  document.head.appendChild(tag);
}

function GlowCanvas() {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return undefined;
    const gl = canvas.getContext('webgl', {
      alpha: true,
      premultipliedAlpha: true,
      antialias: false,
    });
    if (!gl) return undefined;

    const compile = (type: number, src: string): WebGLShader | null => {
      const shader = gl.createShader(type);
      if (!shader) return null;
      gl.shaderSource(shader, src);
      gl.compileShader(shader);
      if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
        console.error('[LessComputerGlow] shader compile failed:', gl.getShaderInfoLog(shader));
        gl.deleteShader(shader);
        return null;
      }
      return shader;
    };

    const vs = compile(gl.VERTEX_SHADER, VERTEX_SRC);
    const fs = compile(gl.FRAGMENT_SHADER, GLOW_FRAGMENT_SRC);
    if (!vs || !fs) return undefined;
    const program = gl.createProgram();
    if (!program) return undefined;
    gl.attachShader(program, vs);
    gl.attachShader(program, fs);
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      console.error('[LessComputerGlow] program link failed:', gl.getProgramInfoLog(program));
      return undefined;
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
    const uRadius = gl.getUniformLocation(program, 'uRadius');

    let raf = 0;
    const start = performance.now();
    const frame = (now: number) => {
      const scale = Math.min(window.devicePixelRatio || 1, 2) * RENDER_SCALE;
      const w = Math.max(1, Math.round(canvas.clientWidth * scale));
      const h = Math.max(1, Math.round(canvas.clientHeight * scale));
      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w;
        canvas.height = h;
        gl.viewport(0, 0, w, h);
      }
      gl.uniform2f(uResolution, w, h);
      gl.uniform1f(uTime, (now - start) / 1000);
      if (uRadius) gl.uniform1f(uRadius, SCREEN_CORNER_RADIUS * scale);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);

    return () => {
      cancelAnimationFrame(raf);
      gl.deleteBuffer(buffer);
      gl.deleteProgram(program);
      gl.deleteShader(vs);
      gl.deleteShader(fs);
      gl.getExtension('WEBGL_lose_context')?.loseContext();
    };
  }, []);

  return <canvas ref={canvasRef} aria-hidden />;
}

export function LessComputerGlow() {
  // issue #470: after the window is .hide()d, the webview doesn't stop its animation loop
  // on its own and the fullscreen glow layer keeps occupying the GPU. The backend drives
  // this by actively emitting visibility on show/hide: when invisible, unmount entirely
  // (rAF stops + GL released → zero GPU); when visible, remount — zero visual change on
  // show.
  const [active, setActive] = useState(true);
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void import('@tauri-apps/api/event').then(({ listen }) => {
      if (cancelled) return;
      void listen<boolean>('less-computer-glow:active', (e) => {
        setActive(Boolean(e.payload));
      }).then((un) => {
        if (cancelled) un();
        else unlisten = un;
      });
    });
    // Secondary fallback: stop when the page is marked hidden too (stop-only, never
    // mistakenly disables a currently shown glow).
    const onVisibility = () => {
      if (document.hidden) setActive(false);
    };
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      cancelled = true;
      unlisten?.();
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, []);

  if (!active) return null;
  return (
    <div className="lcg-root" aria-hidden>
      <GlowCanvas />
    </div>
  );
}
