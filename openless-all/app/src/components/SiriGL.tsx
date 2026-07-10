// SiriGL.tsx — Apple 新版 Siri 动画的 WebGL 复刻，胶囊「纯光效」形态的渲染核心。
//
// Shader 移植自 github.com/aaaa-zhen/siri-glsl（MIT，siri-wave.html）：
//   - wave：siriWaveCore 光谱声波 —— 4 次光谱采样色散 + Lorentzian 发光线。
//     原版 RESOLVED / 假音频信号改为 uniform：uLevel 接真实麦克风电平（capsule:state
//     的 60Hz RMS），uResolved 1→0 时波形向圆心汇聚成呼吸光点（录音 → 思考的过渡）。
//     原版末尾 `col *= res` 会让 res=0 全黑，这里改成保底亮度让汇聚后的光点常亮。
//   - orb：siriFluidDotsCore 流体圆点 —— 6 个 metaball smooth-min 融合 + 周期
//     聚拢/爆发，即官方「思考中」的圆形图标，原样移植。
//
// 渲染框架与 demo 一致：单个大三角形铺满 + fragment shader，uniform 仅 4 个。
// 输出加了 alpha（取 RGB 最大分量），光效直接浮在透明窗口上，无需底色。
// 生命周期：挂载启动 rAF，卸载 cancel + 释放 GL 资源（不可见即零 GPU，#470 同款原则）。

import { useEffect, useRef, type CSSProperties } from 'react';

export type SiriGLMode = 'wave' | 'orb';

interface SiriGLProps {
  mode: SiriGLMode;
  /** wave 专用：原始 RMS 电平 0..1，内部做门限 + 攻快释慢平滑。 */
  level?: number;
  /** wave 专用：true=波形展开（录音），false=向圆心汇聚成呼吸光点（思考）。 */
  resolved?: boolean;
  /**
   * wave 专用：预备态。true = 麦克风尚未就绪，光条走柔和「待命」呼吸（忽略真实电平），
   * 暗示用户稍候；麦克风就绪后置 false，改接真实电平，光条「点亮」进入正式录音。
   */
  warming?: boolean;
  /** wave 专用：预备态预期时长（ms），驱动展开动画的预测节奏。见 Capsule warmupMs。 */
  warmupMs?: number;
  /**
   * 动画时间倍率（默认 1）。shader 时间由内部按 dt×speed 累积而非取墙钟，
   * 因此变速是连续的：思考中 >1 加速转动，收尾回落到标准速度。
   */
  speed?: number;
  /** orb 专用：true = 六点向圆心合并成一颗圆（插入完成的收尾动画）。 */
  merging?: boolean;
  className?: string;
  style?: CSSProperties;
}

/** 内部渲染分辨率倍率（乘在 devicePixelRatio 上），demo 同款取值，越低越省 GPU。 */
const RENDER_SCALE = 0.75;

/**
 * 预备态光条的「收拢度」：wave 的 uResolved 停在这个低值 —— 光条向中心聚拢、明显未展开
 * （视觉上就是「加载中，还没好」）。麦克风就绪后 uResolved 升到 1，光条展开点亮成完整
 * 声波，作为「准备完成、可以开口」的唯一信号。取值偏低才够「未成形」，但不取 0（0 是思
 * 考态的中心光点，避免撞脸）。
 */
const WARMING_RESOLVED = 0.2;

// 导出给 chat/orbFeed（共享 orb 渲染源：一个 GL context 镜像到多个小头像）。
export const VERTEX_SRC = 'attribute vec2 aPos; void main(){ gl_Position=vec4(aPos,0.0,1.0); }';

const WAVE_FRAGMENT_SRC = `
precision highp float;
uniform vec2 iResolution; uniform float iTime;
uniform float uResolved;
uniform float uLevel;
const float PI = 3.14159265359;
const float AMPLITUDE=0.32, FREQ=1.1, ABER_FREQ=1.0, SPEED=2.4, WAVE_SCALE=0.6;
const float ABERRATION=2.6, THICKNESS=3.0, INTENSITY=2., FALLOFF=1.7;
const float EDGE_MASK=0.4, EDGE_INSET=0.0, BAND_FILL=30000.0, BAND_THICK=0.08, SOFTNESS=2.5;
const float LOW_AMP=6.0, LOW_INT=1.5, MID_ABER=0.8, MID_ABAMP=0.05, MID_SOFT=0.4;
const float HIGH_ABER=0.5, HIGH_ABAMP=0.06, UNRES_SCALE=0.14;
vec3 spectral4(int s){ float x=float(s);
  return clamp(vec3(abs(x-3.0)-1.0, 2.0-abs(x-2.0), 2.0-abs(x-4.0)), 0.0, 1.0); }
void main(){
  vec2 R=iResolution.xy; float aspect=R.x/R.y;
  vec2 p=(gl_FragCoord.xy+0.5)*2.0/R-1.0; p.x*=aspect;
  float yScreen=p.y; p/=max(WAVE_SCALE,0.1);
  float t=iTime;
  float dv=clamp(uLevel,0.0,1.0);
  float low =clamp(0.45+0.45*sin(t*0.8)*sin(t*0.37+1.0),0.0,1.0)*dv;
  float mid =clamp(0.40+0.40*sin(t*1.7+2.0)*sin(t*0.53),0.0,1.0)*dv;
  float high=clamp(0.30+0.30*sin(t*2.9+4.0)*sin(t*0.71+2.0),0.0,1.0)*dv;
  float res=clamp(uResolved,0.0,1.0);
  float drift=mod(t,20.0*PI)*SPEED;
  /* 汇聚 = 「四周向中间收缩」（用户拍板）：波形坐标随 res 下降横向放大，
     可见波纹被从两端挤向圆心，而不是原地塌扁 + 相位继续往两边跑（脉冲感）。
     光点距离场仍用未收缩的 p，保持正圆。 */
  vec2 pw=p; pw.x*=mix(5.0,1.0,res);
  float xN=pw.x/max(aspect,1.0);
  float env=cos(PI*0.5*min(abs(0.9*xN),1.0)); env*=env;
  float A1=(AMPLITUDE*mix(0.14,1.0,dv))+0.01*low*LOW_AMP;
  float A2=A1+mid*MID_ABAMP+high*HIGH_ABAMP;
  float AB=(ABERRATION+mid*MID_ABER+high*HIGH_ABER)*res;
  float th=mix(0.1,0.01*THICKNESS,res);
  float inten=mix(0.1,0.01*(INTENSITY+low*LOW_INT),res);
  float soft=0.01*res*max(0.0,SOFTNESS+mid*MID_SOFT);
  float dUnres=max(length(p)-mix(0.14,UNRES_SCALE,res),0.0);
  float yMain=A1*env*res*sin(pw.x*FREQ+drift);
  float bandFillTh=max(BAND_THICK,1e-4);
  float bandAmt=1e-4*BAND_FILL*inten;
  vec3 num=vec3(0.0),den=vec3(0.0);
  for(int s=0;s<4;s++){
    vec3 hue=mix(vec3(1.0),spectral4(s),res); den+=hue;
    float ab=mix(-AB,AB,float(s)/3.0);
    float yL=A2*env*res*sin(pw.x*ABER_FREQ+drift+ab);
    float d=mix(dUnres,abs(p.y-yL),res);
    float lor=mix(1.0/(1.0+(0.02*d)*(0.02*d)),1.0,res);
    float line=inten/(sqrt(d*d+soft*soft)+th);
    float lo=min(yMain,yL),hi=max(yMain,yL);
    float dBand=max(0.0,max(p.y-hi,lo-p.y));
    float band=bandAmt/(dBand+bandFillTh);
    num+=hue*lor*(line+band);
  }
  vec3 col=num/den;
  float dM=mix(dUnres,abs(p.y-yMain),res);
  float lorM=mix(1.0/(1.0+(0.02*dM)*(0.02*dM)),1.0,res);
  /* 原版 boost=(1-res)*(14*low+4)：汇聚光点的大光晕会铺满透明窗口、在窗口
     边界被硬裁出直边。压到含蓄水平 —— 转场一切向内收，不向外扩。 */
  float boost=(1.0-res)*(3.0*low+1.2);
  col+=0.5*inten*(lorM+boost)/(sqrt(dM*dM+soft*soft)+th);
  col=pow(max(col,0.0),vec3(1.5));
  float emT=clamp((abs(yScreen)-1.0+EDGE_INSET)/(-max(EDGE_MASK,1e-4)),0.0,1.0);
  float em=emT*emT*(3.0-2.0*emT);
  float gauss=exp(-pow(xN*FALLOFF,2.0));
  /* 遮罩全程生效（原版 res→0 时遮罩失效会让收拢中段的残波漏到两侧）。 */
  col*=em*gauss;
  col*=mix(0.55,1.0,res);
  float a=clamp(max(col.r,max(col.g,col.b)),0.0,1.0);
  gl_FragColor=vec4(col,a);
}`;

// 相对原版 siriFluidDotsCore 的两处刻意修改（用户拍板）：
//   1. 删除 12s 周期的 gather→burst 爆发 —— 冲击波会顶到方形画布边缘被硬裁；
//      思考态只保留纯粹的转动 + metaball 融合。
//   2. 聚拢改为 uniform uGather 驱动的「出场」：1=六点全聚在圆心（视觉上就是
//      wave 汇聚后的那颗光点），0=散开成环 —— 光条收拢的光点由此平滑「化开」
//      成思考圆点，中间没有任何跳变或弹入冲击。
export const ORB_FRAGMENT_SRC = `
precision highp float;
uniform vec2 iResolution; uniform float iTime;
uniform float uGather;
const float TAU=6.28318530718;
const int N=6;
const float SMOOTH_K=0.08, INTENSITY=0.0025, FALLOFF_P=1.35, FADE_START=0.02, FADE_END=0.56;
const float ABERR=0.005; const vec3 SPECTRAL=vec3(0.0,0.5,1.0)*ABERR;
const float HUE_SPEED=0.06, COLOR_K=0.5, SAT=0.01, HUE_SPAN=0.667;
const float MERGE_PERIOD=6.0, STAGGER=0.33, HOLD=0.0;
const float W=4.6, L=3.2, PIERCE=0.12, RECOIL=0.035, REC_LAG=0.11;
const float GATHER_R=0.008, GATHER_DIM=0.85;
float hash11(float n){ return fract(sin(n*127.1+311.7)*43758.5453); }
float settleWL(float tau,float w,float l){ if(tau<=0.0) return 0.0; return 1.0-exp(-l*tau)*cos(w*tau); }
float settle(float tau){ return settleWL(tau,W,L); }
float smin(float a,float b,float k){ float h=max(k-abs(a-b),0.0)/k; return min(a,b)-h*h*k*0.25; }
vec3 hue2rgb(float h){ h=fract(h);
  float r=clamp(abs(h*6.0-3.0)-1.0,0.0,1.0);
  float g=clamp(2.0-abs(h*6.0-2.0),0.0,1.0);
  float b=clamp(2.0-abs(h*6.0-4.0),0.0,1.0);
  return vec3(r,g,b); }
float dotR(float fi,float seed,float t){ return 0.036+0.010*sin(t*1.3+seed*TAU)+0.005*sin(t*2.4+fi*1.3); }
float dotSD(vec2 p,vec2 pos,float r,float t,float fi,float shapeDamp){
  vec2 d=p-pos;
  float sq=0.075*(0.5+0.5*sin(t*0.9+fi*2.0))*shapeDamp;
  float ca=cos(t*0.35+fi),sa=sin(t*0.35+fi);
  d=mat2(ca,-sa,sa,ca)*d;
  d*=vec2(1.0+sq,1.0-sq);
  return length(d)-r; }
vec3 scene(vec2 p,float t){
  float k=floor(t/MERGE_PERIOD);
  float u=fract(t/MERGE_PERIOD);
  float te=u*MERGE_PERIOD;
  float gC=clamp(uGather,0.0,1.0);
  float gBright=mix(1.0,GATHER_DIM,gC)*(1.0+0.30*gC);
  vec3 total3=vec3(1e5);
  vec3 cAcc=vec3(0.0);
  float wAcc=1e-6;
  for(int i=0;i<N;i++){
    float fi=float(i);
    float seed=hash11(fi);
    float ang=fi/float(N)*TAU+t*0.35;
    vec2 dir=vec2(cos(ang),sin(ang));
    float R=0.17+0.010*sin(t*1.0)+0.007*sin(t*1.3+seed*TAU);
    float pairId=mod(fi,3.0);
    float moverLow=mod(k+pairId,2.0);
    float isMover=(fi<2.5)?step(moverLow,0.5):step(0.5,moverLow);
    float goStart=pairId*STAGGER;
    float retStart=3.0*STAGGER+HOLD+pairId*STAGGER;
    float m=(settle(te-goStart)-settle(te-retStart))*isMover;
    float rec=(settle(te-goStart-REC_LAG)-settle(te-retStart-REC_LAG))*(1.0-isMover);
    float rSelf=dotR(fi,seed,t);
    rSelf=mix(rSelf,0.036,gC);
    float fj=mod(fi+3.0,6.0);
    float rPart=dotR(fj,hash11(fj),t);
    float deep=-(R+RECOIL)-PIERCE*rPart;
    float radial=mix(R,deep,m)+RECOIL*rec;
    radial=mix(radial,GATHER_R,gC);
    vec2 pos=radial*dir;
    float sdR=dotSD(p-SPECTRAL.r*dir,pos,rSelf,t,fi,1.0-gC);
    float sdG=dotSD(p-SPECTRAL.g*dir,pos,rSelf,t,fi,1.0-gC);
    float sdB=dotSD(p-SPECTRAL.b*dir,pos,rSelf,t,fi,1.0-gC);
    total3=vec3(smin(total3.r,sdR,SMOOTH_K),
                smin(total3.g,sdG,SMOOTH_K),
                smin(total3.b,sdB,SMOOTH_K));
    float hue=fract(fi/float(N)+t*HUE_SPEED)*HUE_SPAN;
    vec3 dotCol=mix(vec3(1.0),hue2rgb(hue),SAT);
    float w=exp(-sdG*COLOR_K);
    cAcc+=w*dotCol;
    wAcc+=w;
  }
  vec3 sd3=max(total3,vec3(0.0))+1e-4;
  vec3 core3=clamp(INTENSITY/pow(sd3,vec3(FALLOFF_P)),0.0,1.0);
  vec3 edge3=1.0-smoothstep(vec3(FADE_START),vec3(FADE_END),sd3);
  vec3 bright=core3*edge3*gBright;
  return bright*(cAcc/wAcc);
}
void main(){
  vec2 res=iResolution.xy;
  vec2 p=(2.0*gl_FragCoord.xy-res)/min(res.x,res.y);
  float t=iTime;
  p/=1.0+0.03*sin(t*1.0);
  vec3 col=scene(p,t);
  col*=1.0+0.05*sin(t*1.0+1.0);
  col=pow(col,vec3(1.0/1.2));
  col=min(col,1.0);
  float a=clamp(max(col.r,max(col.g,col.b)),0.0,1.0);
  gl_FragColor=vec4(col,a);
}`;

let webglProbe: boolean | null = null;

/** 一次性探测 WebGL 可用性；失败时调用方退回旧胶囊 UI。 */
export function isWebGLAvailable(): boolean {
  if (webglProbe != null) return webglProbe;
  try {
    const canvas = document.createElement('canvas');
    webglProbe = canvas.getContext('webgl') != null;
  } catch {
    webglProbe = false;
  }
  return webglProbe;
}

let shadersWarmed = false;

/**
 * 空闲预热：在 1×1 的离屏 context 里把两个 shader 各编译一次然后立即释放。
 * GPU 驱动（Metal / ANGLE）按源码缓存编译产物，正式挂载时命中缓存，编译从
 * 几十毫秒降到近零 —— 消除「按下热键后光条慢一拍」的首次延迟。
 */
export function warmUpSiriShaders(): void {
  if (shadersWarmed || !isWebGLAvailable()) return;
  shadersWarmed = true;
  try {
    const canvas = document.createElement('canvas');
    canvas.width = 1;
    canvas.height = 1;
    const gl = canvas.getContext('webgl');
    if (!gl) return;
    for (const src of [WAVE_FRAGMENT_SRC, ORB_FRAGMENT_SRC]) {
      const vs = gl.createShader(gl.VERTEX_SHADER);
      const fs = gl.createShader(gl.FRAGMENT_SHADER);
      const program = gl.createProgram();
      if (!vs || !fs || !program) continue;
      gl.shaderSource(vs, VERTEX_SRC);
      gl.compileShader(vs);
      gl.shaderSource(fs, src);
      gl.compileShader(fs);
      gl.attachShader(program, vs);
      gl.attachShader(program, fs);
      gl.linkProgram(program);
      gl.deleteProgram(program);
      gl.deleteShader(vs);
      gl.deleteShader(fs);
    }
    gl.getExtension('WEBGL_lose_context')?.loseContext();
  } catch {
    // 预热失败无碍：正式挂载时照常编译。
  }
}

/** AudioBars 同款心理声学映射：门限静音底噪 + smoothstep + pow 提升小音量表现。 */
function visualVoice(raw: number): number {
  const gate = 0.012;
  const ceiling = 0.34;
  const gated = Math.min(1, Math.max(0, (raw - gate) / (ceiling - gate)));
  const eased = gated * gated * (3 - 2 * gated);
  return Math.pow(eased, 0.42);
}

export function SiriGL({ mode, level, resolved, warming, warmupMs, speed, merging, className, style }: SiriGLProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  // 60Hz 的 level 更新走 ref 桥接：render 只同步数值，绘制循环在 rAF 里读，
  // 不因 props 变化重建 GL 管线。
  const levelRef = useRef(0);
  const resolvedRef = useRef(1);
  const speedRef = useRef(1);
  const mergingRef = useRef(0);
  const warmingRef = useRef(0);
  const warmupMsRef = useRef(150);
  levelRef.current = Math.min(1, Math.max(0, level ?? 0));
  resolvedRef.current = resolved === false ? 0 : 1;
  speedRef.current = speed ?? 1;
  mergingRef.current = merging === true ? 1 : 0;
  warmingRef.current = warming === true ? 1 : 0;
  warmupMsRef.current = warmupMs ?? 150;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return undefined;
    // 每次 effect 都新建 canvas：卸载时我们主动 loseContext 释放 GPU，而浏览器对
    // 同一个 canvas 永远返回同一个（已死的）context —— React 复用 DOM 节点时
    // （StrictMode 双挂载 / mode 切换重跑 effect）在旧 canvas 上重建管线必然全军
    // 覆没（compile failed: null）。新 canvas = 新 context，彻底绕开。
    const canvas = document.createElement('canvas');
    canvas.style.cssText = 'display:block;width:100%;height:100%;pointer-events:none;';
    host.appendChild(canvas);
    const gl = canvas.getContext('webgl', {
      alpha: true,
      premultipliedAlpha: true,
      antialias: false,
    });
    if (!gl) {
      canvas.remove();
      return undefined;
    }

    const compile = (type: number, src: string): WebGLShader | null => {
      const shader = gl.createShader(type);
      if (!shader) return null;
      gl.shaderSource(shader, src);
      gl.compileShader(shader);
      if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
        console.error('[SiriGL] shader compile failed:', gl.getShaderInfoLog(shader));
        gl.deleteShader(shader);
        return null;
      }
      return shader;
    };

    const vs = compile(gl.VERTEX_SHADER, VERTEX_SRC);
    const fs = compile(gl.FRAGMENT_SHADER, mode === 'wave' ? WAVE_FRAGMENT_SRC : ORB_FRAGMENT_SRC);
    if (!vs || !fs) {
      canvas.remove();
      return undefined;
    }
    const program = gl.createProgram();
    if (!program) {
      canvas.remove();
      return undefined;
    }
    gl.attachShader(program, vs);
    gl.attachShader(program, fs);
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      console.error('[SiriGL] program link failed:', gl.getProgramInfoLog(program));
      canvas.remove();
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
    const uResolvedLoc = gl.getUniformLocation(program, 'uResolved');
    const uLevelLoc = gl.getUniformLocation(program, 'uLevel');
    const uGatherLoc = gl.getUniformLocation(program, 'uGather');

    // 绘制侧的缓动状态：电平攻快释慢（VU 表手感），resolved 指数趋近（「慢慢汇聚」）。
    // shader 时间按 dt×speed 累积（非墙钟），变速连续无跳变。
    // orb 的 uGather 完整生命周期（用户拍板）：出场 gather=1 全聚圆心（接住 wave
    // 收缩成的光点）→ 稳住一拍后缓缓「从中心散开」成环转动 → merging=true 时
    // 干脆地（k=4）合并回中央一颗圆，随淡出消失。
    const GATHER_HOLD_S = 0.3;
    let smoothLevel = 0;
    // 预备态一挂载就从收拢态起步（未成形=加载中），避免「先展开一下又收拢」的突兀；
    // 麦克风就绪后再展开到 resolvedRef。
    let smoothResolved = warmingRef.current > 0.5 ? WARMING_RESOLVED : resolvedRef.current;
    // 预测式展开进度（0=收拢, 1=展开）：warming 入场从 0 起爬，非 warming 直接 1。
    let warmProgress = warmingRef.current > 0.5 ? 0 : 1;
    let smoothSpeed = speedRef.current;
    let gather = 1;
    let shaderTime = 0;
    let elapsed = 0;
    let raf = 0;
    let last = performance.now();

    const frame = (now: number) => {
      const dt = Math.min(0.05, (now - last) / 1000);
      last = now;
      elapsed += dt;
      smoothSpeed += (speedRef.current - smoothSpeed) * (1 - Math.exp(-dt * 2.5));
      shaderTime += dt * smoothSpeed;
      const t = shaderTime;

      const dpr = Math.min(window.devicePixelRatio || 1, 2) * RENDER_SCALE;
      const w = Math.max(1, Math.round(canvas.clientWidth * dpr));
      const h = Math.max(1, Math.round(canvas.clientHeight * dpr));
      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w;
        canvas.height = h;
        gl.viewport(0, 0, w, h);
      }

      // 汇聚态（resolved→0）时电平失去含义，切换成含蓄的慢呼吸信号驱动光点搏动。
      const thinking = resolvedRef.current < 0.5;
      const warmingNow = warmingRef.current > 0.5;
      // 电平：预备态走低幅「加载脉动」（暗、慢，明显未就绪），思考态呼吸，录音态接真实电平。
      const target = warmingNow
        ? 0.12 + 0.06 * Math.sin(t * 3.0)
        : thinking
          ? 0.14 + 0.07 * Math.sin(t * 2.2)
          : visualVoice(levelRef.current);
      const attack = target > smoothLevel ? 14 : 5;
      smoothLevel += (target - smoothLevel) * (1 - Math.exp(-dt * attack));
      // 预测式展开（用户方案）：入场展开不再死等就绪信号（那会「亮起→卡一下→突然展开」），
      // 而是按历史平均加载时长 warmupMs 从按下就平滑推进：
      //   · 未就绪（warming）：warmProgress 按 warmupMs 匀速慢爬，cap 0.9（留一截给就绪收尾）；
      //   · 就绪（warming→false）：快速补到 1 —— 就绪早=剩得多，视觉上「迅速展开完成」；
      //   · 加载超长：停在 0.9 等着（用户说的「没办法」的情况），光条明显未满=还没好。
      const warmupSec = Math.max(0.06, warmupMsRef.current / 1000);
      if (warmingNow) {
        warmProgress = Math.min(0.9, warmProgress + dt / warmupSec);
      } else {
        warmProgress += (1 - warmProgress) * (1 - Math.exp(-dt * 9));
      }
      // resolved：思考态（resolvedRef→0）走原「从容汇聚」；入场/录音由 warmProgress 从收拢
      // （WARMING_RESOLVED）展开到满，smoothResolved 紧跟（展开节奏已由 warmProgress 控速）。
      const targetResolved = thinking
        ? resolvedRef.current
        : WARMING_RESOLVED + (1 - WARMING_RESOLVED) * warmProgress;
      const resolvedK = thinking ? 2.2 : 9.0;
      smoothResolved += (targetResolved - smoothResolved) * (1 - Math.exp(-dt * resolvedK));
      // 出场 hold 之后才开始散开；合并（目标 1）快、散开（目标 0）缓。
      if (mergingRef.current === 1) {
        gather += (1 - gather) * (1 - Math.exp(-dt * 4.0));
      } else if (elapsed > GATHER_HOLD_S) {
        gather += (0 - gather) * (1 - Math.exp(-dt * 1.6));
      }

      gl.uniform2f(uResolution, w, h);
      gl.uniform1f(uTime, t);
      if (uResolvedLoc) gl.uniform1f(uResolvedLoc, smoothResolved);
      if (uLevelLoc) gl.uniform1f(uLevelLoc, smoothLevel);
      if (uGatherLoc) gl.uniform1f(uGatherLoc, gather);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
      raf = requestAnimationFrame(frame);
    };
    // 首帧同步绘制（不等下一个 vsync）：挂载即有画面，热键按下的响应少一帧延迟。
    frame(performance.now());

    return () => {
      cancelAnimationFrame(raf);
      gl.deleteBuffer(buffer);
      gl.deleteProgram(program);
      gl.deleteShader(vs);
      gl.deleteShader(fs);
      gl.getExtension('WEBGL_lose_context')?.loseContext();
      canvas.remove();
    };
  }, [mode]);

  return (
    <div
      ref={hostRef}
      className={className}
      style={{ display: 'block', pointerEvents: 'none', ...style }}
      aria-hidden
    />
  );
}
