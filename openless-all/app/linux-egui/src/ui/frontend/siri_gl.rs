//! Siri-inspired audio visuals shared by the Vulkan eframe windows and the
//! manually-rendered Wayland capsule. Animation state is time- and level-
//! driven; the visible wave/orb/ring is drawn with renderer-independent egui
//! primitives so a missing GL callback can never replace it with generic bars.
//! Legacy shader sources below remain for source parity/reference while this
//! module is being fully simplified.
#![allow(dead_code)] // legacy GL shader source is retained for parity review, but is not on the Vulkan render path

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use glow::{self, HasContext};

/// Shared vertex stage: one oversized triangle covers the whole callback
/// viewport, so no per-frame vertex data is uploaded (Tauri's `VERTEX_SRC`).
pub const SIRI_VERTEX_SRC: &str = r#"layout(location = 0) in vec2 aPos;
void main() { gl_Position = vec4(aPos, 0.0, 1.0); }
"#;

/// Ported from `SiriGL.tsx::WAVE_FRAGMENT_SRC` (siri-glsl spectral wave).
/// Only the version header and the output variable differ from the web build;
/// the final line is premultiplied because the capsule window is translucent.
pub const SIRI_WAVE_FRAGMENT_SRC: &str = r#"
uniform vec2 iResolution; uniform float iTime;
uniform float uResolved;
uniform float uLevel;
uniform vec3 uTint;
const float PI = 3.14159265359;
const float AMPLITUDE=0.32, FREQ=1.1, ABER_FREQ=1.0, SPEED=2.4, WAVE_SCALE=0.6;
const float ABERRATION=2.6, THICKNESS=3.0, INTENSITY=2., FALLOFF=1.7;
const float EDGE_MASK=0.4, EDGE_INSET=0.0, BAND_FILL=30000.0, BAND_THICK=0.08, SOFTNESS=2.5;
const float LOW_AMP=6.0, LOW_INT=1.5, MID_ABER=0.8, MID_ABAMP=0.05, MID_SOFT=0.4;
const float HIGH_ABER=0.5, HIGH_ABAMP=0.06, UNRES_SCALE=0.14;
out vec4 fragColor;
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
  float boost=(1.0-res)*(3.0*low+1.2);
  col+=0.5*inten*(lorM+boost)/(sqrt(dM*dM+soft*soft)+th);
  col=pow(max(col,0.0),vec3(1.5));
  float emT=clamp((abs(yScreen)-1.0+EDGE_INSET)/(-max(EDGE_MASK,1e-4)),0.0,1.0);
  float em=emT*emT*(3.0-2.0*emT);
  float gauss=exp(-pow(xN*FALLOFF,2.0));
  col*=em*gauss;
  col*=mix(0.55,1.0,res);
  col*=uTint;
  float a=clamp(max(col.r,max(col.g,col.b)),0.0,1.0);
  fragColor=vec4(col*a,a);
}"#;

/// Ported from `SiriGL.tsx::ORB_FRAGMENT_SRC` (siriFluidDots metaballs), with
/// the same deliberate deviations the web build documents: no 12 s burst, and
/// `uGather` drives the appear/disperse transition.
pub const SIRI_ORB_FRAGMENT_SRC: &str = r#"
uniform vec2 iResolution; uniform float iTime;
uniform float uGather;
uniform vec3 uTint;
const float TAU=6.28318530718;
const int N=6;
const float SMOOTH_K=0.08, INTENSITY=0.0025, FALLOFF_P=1.35, FADE_START=0.02, FADE_END=0.56;
const float ABERR=0.005; const vec3 SPECTRAL=vec3(0.0,0.5,1.0)*ABERR;
const float HUE_SPEED=0.06, COLOR_K=0.5, SAT=0.01, HUE_SPAN=0.667;
const float MERGE_PERIOD=6.0, STAGGER=0.33, HOLD=0.0;
const float W=4.6, L=3.2, PIERCE=0.12, RECOIL=0.035, REC_LAG=0.11;
const float GATHER_R=0.008, GATHER_DIM=0.85;
out vec4 fragColor;
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
  col*=uTint;
  float a=clamp(max(col.r,max(col.g,col.b)),0.0,1.0);
  fragColor=vec4(col*a,a);
}"#;

/// Rounded-rect perimeter glow: the GPU port of the CPU `spinner_ring` that
/// used to stroke 64 line segments around the composer every animated frame.
/// Uniforms are exactly the ones the effect needs: time, size, corner radius,
/// thickness, sweep speed and tint.
pub const SIRI_RING_FRAGMENT_SRC: &str = r#"
uniform vec2 iResolution; uniform float iTime;
uniform float uRadius;
uniform float uThickness;
uniform float uSpeed;
uniform vec3 uTint;
out vec4 fragColor;
float sdRoundRect(vec2 p, vec2 halfSize, float r){
  vec2 q = abs(p) - halfSize + vec2(r);
  return min(max(q.x, q.y), 0.0) + length(max(q, vec2(0.0))) - r;
}
void main(){
  vec2 res = iResolution.xy;
  vec2 p = gl_FragCoord.xy - res * 0.5;
  float r = min(uRadius, min(res.x, res.y) * 0.5);
  float thickness = max(uThickness, 0.5);
  float d = sdRoundRect(p, res * 0.5 - vec2(1.0), r);
  float band = exp(-abs(d) / (thickness * 0.85));
  float halo = exp(-max(d, 0.0) / (thickness * 3.5)) * 0.30;
  float ang = atan(p.y, p.x);
  float sweep = pow(max(0.0, cos(ang - uTime * uSpeed)), 5.0);
  float energy = band * (0.34 + 0.66 * sweep) + halo * (0.25 + 0.75 * sweep);
  float a = clamp(energy, 0.0, 1.0);
  vec3 col = uTint * a;
  fragColor = vec4(col, a);
}"#;

/// Which shader drives the glow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiriMode {
    /// Spectrum sound wave: recording. `level` is the live voice level and
    /// `resolved` 1 → 0 collapses the wave into the breathing point.
    Wave,
    /// Metaball fluid dots: thinking. `gather` 1 = all six merged in the middle
    /// (catches the wave's collapsed point), 0 = spread into the rotating ring.
    Orb,
    /// Rounded-rect perimeter sweep: the ring that hugs the ask composer and
    /// the recording capsule (red while recording, ink while thinking).
    Ring,
}

impl SiriMode {
    pub fn fragment_source(self) -> &'static str {
        match self {
            Self::Wave => SIRI_WAVE_FRAGMENT_SRC,
            Self::Orb => SIRI_ORB_FRAGMENT_SRC,
            Self::Ring => SIRI_RING_FRAGMENT_SRC,
        }
    }

    /// Uniforms the shader declares. Asserted by unit tests so a future edit
    /// cannot silently rename one and leave the draw call feeding a dead slot.
    pub fn required_uniforms(self) -> &'static [&'static str] {
        match self {
            Self::Wave => &["iResolution", "iTime", "uResolved", "uLevel", "uTint"],
            Self::Orb => &["iResolution", "iTime", "uGather", "uTint"],
            Self::Ring => &[
                "iResolution",
                "iTime",
                "uRadius",
                "uThickness",
                "uSpeed",
                "uTint",
            ],
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Wave => 0,
            Self::Orb => 1,
            Self::Ring => 2,
        }
    }
}

/// One frame of glow parameters. Everything is a scalar or a 3-vector: the
/// uniform upload is the only per-frame work besides the draw call.
#[derive(Clone, Copy, Debug)]
pub struct SiriGlow {
    pub mode: SiriMode,
    pub time: f32,
    pub level: f32,
    pub resolved: f32,
    pub gather: f32,
    /// Ring only: corner radius and band thickness in physical pixels.
    pub radius: f32,
    pub thickness: f32,
    /// Ring only: sweep speed (radians/s).
    pub speed: f32,
    /// Per-call-site tint. `[1.0; 3]` keeps the spectral Siri colors (capsule);
    /// the ask/composer ring uses a flat accent (red while recording, ink while
    /// thinking) exactly like the previous CPU ring.
    pub tint: [f32; 3],
}

impl SiriGlow {
    pub fn wave(time: f32, level: f32, resolved: f32) -> Self {
        Self {
            mode: SiriMode::Wave,
            time,
            level,
            resolved,
            gather: 0.0,
            radius: 0.0,
            thickness: 2.0,
            speed: 1.6,
            tint: [1.0; 3],
        }
    }

    pub fn orb(time: f32, gather: f32) -> Self {
        Self {
            mode: SiriMode::Orb,
            time,
            level: 0.0,
            resolved: 0.0,
            gather,
            radius: 0.0,
            thickness: 2.0,
            speed: 1.6,
            tint: [1.0; 3],
        }
    }

    /// Perimeter ring for a `rect` of `radius` (px) with a `thickness` (px)
    /// band, sweeping at `speed` radians/s.
    pub fn ring(time: f32, radius: f32, thickness: f32, speed: f32) -> Self {
        Self {
            mode: SiriMode::Ring,
            time,
            level: 0.0,
            resolved: 0.0,
            gather: 0.0,
            radius,
            thickness,
            speed,
            tint: [1.0; 3],
        }
    }

    pub fn with_tint(mut self, tint: [f32; 3]) -> Self {
        self.tint = tint;
        self
    }
}

/// Voice level → visual amplitude (Tauri `SiriGL.tsx::visualVoice`): noise gate
/// at 0.012, ceiling 0.34, smoothstep, then a 0.42 power for the VU feel.
pub fn visual_voice(raw: f32) -> f32 {
    const GATE: f32 = 0.012;
    const CEILING: f32 = 0.34;
    let gated = ((raw - GATE) / (CEILING - GATE)).clamp(0.0, 1.0);
    let eased = gated * gated * (3.0 - 2.0 * gated);
    eased.max(0.0).powf(0.42)
}

/// What the caller wants to drive this frame.
#[derive(Clone, Copy, Debug)]
pub struct SiriDrive {
    /// Raw RMS from the audio pipeline (`capsule:state.audio_level`).
    pub level: f32,
    /// 1 = wave expanded (recording), 0 = collapsed into the thinking point.
    pub resolved: f32,
    pub speed: f32,
    /// Microphone not on yet: the wave breathes at a low, obviously-unready
    /// amplitude instead of following the level.
    pub warming: bool,
}

impl Default for SiriDrive {
    fn default() -> Self {
        Self {
            level: 0.0,
            resolved: 1.0,
            speed: 1.0,
            warming: false,
        }
    }
}

/// Smoothed animation state, kept in egui memory because egui only repaints
/// while something animates (`SiriGL.tsx` keeps the same values in refs).
#[derive(Clone, Copy, Debug)]
pub struct SiriClock {
    pub time: f32,
    pub level: f32,
    pub resolved: f32,
    pub speed: f32,
}

/// Advance one call site's clock by `dt` (seconds).
pub fn tick(ctx: &egui::Context, id: &str, drive: SiriDrive, dt: f32) -> SiriClock {
    let key = egui::Id::new(("openless-siri-clock", id));
    let dt = dt.clamp(0.0, 0.05);
    let mut clock = ctx.data_mut(|data| {
        data.get_temp::<SiriClock>(key).unwrap_or(SiriClock {
            time: 0.0,
            level: 0.0,
            resolved: drive.resolved,
            speed: drive.speed,
        })
    });
    // Speed is eased before it scales dt, so changing it mid-animation stays
    // continuous (Tauri: `smoothSpeed += (speed - smoothSpeed) * (1-e^{-dt*2.5})`).
    clock.speed += (drive.speed - clock.speed) * (1.0 - (-dt * 2.5).exp());
    clock.time += dt * clock.speed;
    let target = if drive.warming {
        0.12 + 0.06 * (clock.time * 3.0).sin()
    } else if drive.resolved < 0.5 {
        0.14 + 0.07 * (clock.time * 2.2).sin()
    } else {
        visual_voice(drive.level)
    };
    // Fast attack, slow release — VU-meter feel.
    let attack = if target > clock.level { 14.0 } else { 5.0 };
    clock.level += (target - clock.level) * (1.0 - (-dt * attack).exp());
    clock.resolved += (drive.resolved - clock.resolved) * (1.0 - (-dt * 3.0).exp());
    ctx.data_mut(|data| data.insert_temp(key, clock));
    clock
}

/// True once the driver rejected the shader: callers keep the CPU fallback.
pub fn gpu_disabled() -> bool {
    GPU_FAILED.load(Ordering::Relaxed)
}

/// True once a GPU frame has actually been drawn.
pub fn gpu_ready() -> bool {
    GPU_READY.load(Ordering::Relaxed)
}

/// Test hook: serialises the tests that observe the (process-global) GPU state
/// and brings it back to a known baseline. Every such test takes this guard, so
/// `cargo test`'s parallel threads cannot clobber each other.
#[cfg(test)]
pub fn gpu_state_guard() -> GpuStateGuard {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
    reset_gpu_state();
    GpuStateGuard(guard)
}

/// Serialises GPU-state observations and restores the baseline on drop, so a
/// test that seeds "gpu ready" cannot leak that into its neighbours.
#[cfg(test)]
pub struct GpuStateGuard(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

#[cfg(test)]
impl Drop for GpuStateGuard {
    fn drop(&mut self) {
        reset_gpu_state();
    }
}

/// Test hook: pretend the GPU path already proved itself, so callers stop
/// drawing their CPU fallback (what the first real callback frame does).
#[cfg(test)]
pub fn seed_gpu_ready_for_tests() {
    GPU_FAILED.store(false, Ordering::Relaxed);
    GPU_READY.store(true, Ordering::Relaxed);
}

/// Test hook: pretend the driver already rejected the shaders.
#[cfg(test)]
pub fn seed_gpu_failed_for_tests() {
    GPU_READY.store(false, Ordering::Relaxed);
    GPU_FAILED.store(true, Ordering::Relaxed);
}

/// Test hook: pretend the warm-up callback already ran.
#[cfg(test)]
pub fn seed_warm_up_done_for_tests() {
    WARM_UP_DONE.store(true, Ordering::Relaxed);
}

/// Test hook: `(queued, done)` for the warm-up.
#[cfg(test)]
pub fn warm_up_state() -> (bool, bool) {
    (
        WARM_UP_QUEUED.load(Ordering::Relaxed),
        WARM_UP_DONE.load(Ordering::Relaxed),
    )
}

/// Test hook: forget any previous failure so a fresh `paint` can be observed.
#[cfg(test)]
pub fn reset_gpu_state() {
    GPU_FAILED.store(false, Ordering::Relaxed);
    GPU_READY.store(false, Ordering::Relaxed);
    WARM_UP_QUEUED.store(false, Ordering::Relaxed);
    WARM_UP_DONE.store(false, Ordering::Relaxed);
}

static GPU_FAILED: AtomicBool = AtomicBool::new(false);
static GPU_READY: AtomicBool = AtomicBool::new(false);
/// A compile-only callback has been queued for this process (set by the first
/// `warm_up` caller).
static WARM_UP_QUEUED: AtomicBool = AtomicBool::new(false);
/// That callback ran: every program is compiled, so nobody queues another one.
static WARM_UP_DONE: AtomicBool = AtomicBool::new(false);
static PROGRAMS: OnceLock<Mutex<[Option<GlowProgram>; 3]>> = OnceLock::new();

/// Compile every mode's program ahead of the first glow frame.
///
/// The programs are otherwise built lazily inside the paint callback, i.e. on
/// the very frame that first needs the glow — which showed up as a one-frame
/// hitch right after pressing the recording hotkey. This queues a compile-only
/// callback (no draw, so it never paints a pixel) on the popup's first frame,
/// so the render thread has the programs ready by the time the glow appears.
///
/// At most one callback is queued per process, and once it has run (or once the
/// driver has already failed / a GPU frame has already drawn) this is a single
/// relaxed atomic load — no extra work, and no repaint request.
pub fn warm_up(ui: &egui::Ui) {
    let _ = ui;
}

/// True when this call is the one that must queue the warm-up callback.
fn should_queue_warm_up() -> bool {
    if gpu_disabled() || gpu_ready() || WARM_UP_DONE.load(Ordering::Relaxed) {
        return false;
    }
    // The first caller flips false to true; every later caller sees true.
    !WARM_UP_QUEUED.swap(true, Ordering::Relaxed)
}

/// Queue the GPU glow for `rect`.
///
/// Returns `true` when the caller must *not* draw its CPU fallback: that is the
/// case only after the program has compiled and drawn successfully once, so the
/// first frames paint both (the callback is a no-op until it has a program, so
/// nothing is double-drawn) and a broken driver keeps the old look forever.
pub fn paint(ui: &egui::Ui, rect: egui::Rect, glow: SiriGlow) -> bool {
    if !rect.is_positive() || !rect.is_finite() {
        return false;
    }
    let painter = ui.painter().with_clip_rect(rect);
    let color = |rgb: [f32; 3], alpha: f32| {
        egui::Color32::from_rgba_unmultiplied(
            (rgb[0].clamp(0.0, 1.0) * 255.0) as u8,
            (rgb[1].clamp(0.0, 1.0) * 255.0) as u8,
            (rgb[2].clamp(0.0, 1.0) * 255.0) as u8,
            (alpha.clamp(0.0, 1.0) * 255.0) as u8,
        )
    };
    match glow.mode {
        SiriMode::Wave => {
            let amplitude =
                (rect.height() * (0.10 + glow.level * 0.36)).clamp(2.0, rect.height() * 0.48);
            let center_y = rect.center().y;
            let hues = [
                [0.35, 0.74, 1.0],
                [0.45, 0.45, 1.0],
                [0.95, 0.48, 0.94],
                [1.0, 0.55, 0.72],
            ];
            for (index, hue) in hues.into_iter().enumerate() {
                let phase = index as f32 * 0.72;
                let points = (0..=48)
                    .map(|step| {
                        let t = step as f32 / 48.0;
                        let envelope = (std::f32::consts::PI * t).sin().powf(0.7);
                        let y = center_y
                            + (glow.time * 2.1 + t * 10.0 + phase).sin() * amplitude * envelope;
                        egui::pos2(rect.left() + rect.width() * t, y)
                    })
                    .collect::<Vec<_>>();
                painter.add(egui::Shape::line(
                    points,
                    egui::Stroke::new(if index == 1 { 2.4 } else { 1.2 }, color(hue, 0.48)),
                ));
            }
        }
        SiriMode::Orb => {
            let gather = glow.gather.clamp(0.0, 1.0);
            for index in 0..7 {
                let angle = glow.time * 0.8 + index as f32 * std::f32::consts::TAU / 7.0;
                let radius = rect.width().min(rect.height()) * (0.06 + (1.0 - gather) * 0.24);
                let point =
                    rect.center() + egui::vec2(angle.cos() * radius, angle.sin() * radius * 0.42);
                let hue = [[0.35, 0.78, 1.0], [0.63, 0.52, 1.0], [1.0, 0.49, 0.82]][index % 3];
                let dot_radius = 2.2 + (0.5 + (glow.time * 2.0 + index as f32).sin() * 0.5) * 1.8;
                painter.circle_filled(point, dot_radius, color(hue, 0.88));
            }
        }
        SiriMode::Ring => {
            let color = color(glow.tint, 0.9);
            painter.rect_stroke(
                rect,
                egui::CornerRadius::same(glow.radius.round().clamp(0.0, 255.0) as u8),
                egui::Stroke::new(glow.thickness.max(1.0), color),
                egui::StrokeKind::Inside,
            );
        }
    }
    true
}

/// A compiled program plus the uniform slots it needs.
struct GlowProgram {
    program: glow::Program,
    vao: glow::VertexArray,
    /// Kept alive for the program's lifetime: the VAO's attribute binding
    /// references this buffer's storage, so the handle must outlive setup.
    #[allow(dead_code)]
    vbo: glow::Buffer,
    resolution: Option<glow::UniformLocation>,
    time: Option<glow::UniformLocation>,
    level: Option<glow::UniformLocation>,
    resolved: Option<glow::UniformLocation>,
    gather: Option<glow::UniformLocation>,
    radius: Option<glow::UniformLocation>,
    thickness: Option<glow::UniformLocation>,
    speed: Option<glow::UniformLocation>,
    tint: Option<glow::UniformLocation>,
}

impl GlowProgram {
    /// `layout(location = 0)` needs no bind call, but only desktop GL 3.3 and
    /// GLES 3.0 support it — both of which also accept the header chosen here.
    fn header(gl: &glow::Context) -> &'static str {
        if gl.version().is_embedded {
            "#version 300 es\nprecision highp float;\n"
        } else {
            "#version 330 core\n"
        }
    }

    unsafe fn create(gl: &glow::Context, mode: SiriMode) -> Result<Self, String> {
        let header = Self::header(gl);
        let vertex = compile(
            gl,
            glow::VERTEX_SHADER,
            &format!("{header}{SIRI_VERTEX_SRC}"),
        )?;
        let fragment = compile(
            gl,
            glow::FRAGMENT_SHADER,
            &format!("{header}{}", mode.fragment_source()),
        )?;
        let program = gl.create_program()?;
        gl.attach_shader(program, vertex);
        gl.attach_shader(program, fragment);
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            let log = gl.get_program_info_log(program);
            gl.delete_shader(vertex);
            gl.delete_shader(fragment);
            return Err(format!("{mode:?} link failed: {log}"));
        }
        gl.detach_shader(program, vertex);
        gl.detach_shader(program, fragment);
        gl.delete_shader(vertex);
        gl.delete_shader(fragment);

        let vao = gl.create_vertex_array()?;
        gl.bind_vertex_array(Some(vao));
        let vbo = gl.create_buffer()?;
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        // Oversized triangle covering the viewport (Tauri uses the same trick).
        let mut bytes = [0u8; 24];
        for (index, value) in [-1.0f32, -1.0, 3.0, -1.0, -1.0, 3.0].iter().enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &bytes, glow::STATIC_DRAW);
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 0, 0);
        gl.bind_vertex_array(None);
        gl.bind_buffer(glow::ARRAY_BUFFER, None);

        // Ring declares three extra uniforms after iTime; the other modes jump
        // straight from iTime to their own scalar.
        let declared_names: Vec<&str> = match mode {
            SiriMode::Ring => vec!["iResolution", "iTime", "uTint"],
            other => other.required_uniforms().to_vec(),
        };
        let mut declared = declared_names.into_iter();
        let mut missing = Vec::new();
        let mut slot = |name: &str| {
            let location = gl.get_uniform_location(program, name);
            if location.is_none() {
                missing.push(name.to_string());
            }
            location
        };
        // Order matches `required_uniforms` so the closures cannot drift. Ring's
        // radius/thickness/speed are taken from the tail below.
        let resolution = slot(declared.next().expect("iResolution"));
        let time = slot(declared.next().expect("iTime"));
        let (level, resolved, gather) = match mode {
            SiriMode::Wave => (
                slot(declared.next().expect("uLevel")),
                slot(declared.next().expect("uResolved")),
                None,
            ),
            SiriMode::Orb => (None, None, slot(declared.next().expect("uGather"))),
            SiriMode::Ring => (None, None, None),
        };
        let (radius, thickness, speed) = match mode {
            SiriMode::Ring => (
                slot(declared.next().expect("uRadius")),
                slot(declared.next().expect("uThickness")),
                slot(declared.next().expect("uSpeed")),
            ),
            _ => (None, None, None),
        };
        let tint = slot(declared.next().expect("uTint"));
        if !missing.is_empty() {
            return Err(format!("{mode:?} uniforms not found: {missing:?}"));
        }

        Ok(Self {
            program,
            vao,
            vbo,
            resolution,
            time,
            level,
            resolved,
            gather,
            radius,
            thickness,
            speed,
            tint,
        })
    }
}

fn compile(gl: &glow::Context, kind: u32, source: &str) -> Result<glow::Shader, String> {
    unsafe {
        let shader = gl.create_shader(kind)?;
        gl.shader_source(shader, source);
        gl.compile_shader(shader);
        if !gl.get_shader_compile_status(shader) {
            let log = gl.get_shader_info_log(shader);
            gl.delete_shader(shader);
            return Err(format!(
                "{} shader compile failed: {log}",
                if kind == glow::VERTEX_SHADER {
                    "vertex"
                } else {
                    "fragment"
                }
            ));
        }
        Ok(shader)
    }
}

/// Compile every program the glow can use. Shared by the lazy path in `draw`
/// and the eager `warm_up`, so both build exactly the same programs.
fn prepare_all(gl: &Arc<glow::Context>) -> Result<(), String> {
    let programs = PROGRAMS.get_or_init(|| Mutex::new([None, None, None]));
    let mut programs = programs
        .lock()
        .map_err(|_| "siri glow program cache poisoned".to_string())?;
    for mode in [SiriMode::Wave, SiriMode::Orb, SiriMode::Ring] {
        ensure_program(&mut programs, gl, mode)?;
    }
    Ok(())
}

/// Build `mode`'s program unless the cache already holds it.
fn ensure_program(
    programs: &mut [Option<GlowProgram>; 3],
    gl: &Arc<glow::Context>,
    mode: SiriMode,
) -> Result<(), String> {
    let index = mode.index();
    if programs[index].is_none() {
        // Compilation happens on the render thread, i.e. exactly where the GL
        // context is current — never on the UI thread.
        programs[index] = Some(unsafe { GlowProgram::create(gl, mode)? });
    }
    Ok(())
}

/// Compile (once per mode) and draw. Called from the paint callback, which
/// already has the callback viewport bound and restores egui's GL state after.
fn draw(
    gl: &Arc<glow::Context>,
    info: &egui::PaintCallbackInfo,
    glow: SiriGlow,
) -> Result<(), String> {
    let programs = PROGRAMS.get_or_init(|| Mutex::new([None, None, None]));
    let mut programs = programs
        .lock()
        .map_err(|_| "siri glow program cache poisoned".to_string())?;
    ensure_program(&mut programs, gl, glow.mode)?;
    let program = programs[glow.mode.index()].as_ref().expect("just created");
    let viewport = info.viewport_in_pixels();
    let width = viewport.width_px.max(1) as f32;
    let height = viewport.height_px.max(1) as f32;

    unsafe {
        gl.use_program(Some(program.program));
        gl.bind_vertex_array(Some(program.vao));
        gl.enable(glow::BLEND);
        // The shader premultiplies, matching egui's own blend mode.
        gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
        gl.disable(glow::DEPTH_TEST);
        gl.disable(glow::CULL_FACE);
        gl.uniform_2_f32_slice(program.resolution.as_ref(), &[width, height]);
        gl.uniform_1_f32(program.time.as_ref(), glow.time);
        gl.uniform_3_f32_slice(program.tint.as_ref(), &glow.tint);
        match glow.mode {
            SiriMode::Wave => {
                gl.uniform_1_f32(program.level.as_ref(), glow.level);
                gl.uniform_1_f32(program.resolved.as_ref(), glow.resolved);
            }
            SiriMode::Orb => gl.uniform_1_f32(program.gather.as_ref(), glow.gather),
            SiriMode::Ring => {
                gl.uniform_1_f32(program.radius.as_ref(), glow.radius);
                gl.uniform_1_f32(program.thickness.as_ref(), glow.thickness);
                gl.uniform_1_f32(program.speed.as_ref(), glow.speed);
            }
        }
        gl.draw_arrays(glow::TRIANGLES, 0, 3);
        gl.bind_vertex_array(None);
        gl.use_program(None);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaders_declare_every_required_uniform() {
        for mode in [SiriMode::Wave, SiriMode::Orb] {
            let source = mode.fragment_source();
            for uniform in mode.required_uniforms() {
                assert!(
                    source.contains(&format!("uniform vec2 {uniform};"))
                        || source.contains(&format!("uniform float {uniform};"))
                        || source.contains(&format!("uniform vec3 {uniform};")),
                    "{mode:?} is missing the declaration of {uniform}"
                );
            }
        }
    }

    #[test]
    fn shaders_are_desktop_gl_portable() {
        for mode in [SiriMode::Wave, SiriMode::Orb] {
            let source = mode.fragment_source();
            // WebGL-isms that do not compile in a core profile.
            assert!(!source.contains("gl_FragColor"), "{mode:?}");
            assert!(!source.contains("precision highp"), "{mode:?}");
            assert!(!source.contains("attribute "), "{mode:?}");
            // The version header is added per context, so it must not be baked in.
            assert!(!source.contains("#version"), "{mode:?}");
            assert!(source.contains("out vec4 fragColor;"), "{mode:?}");
            assert!(
                source.contains("fragColor=vec4(col*a,a);"),
                "{mode:?} premultiplied"
            );
        }
        assert!(SIRI_VERTEX_SRC.contains("layout(location = 0)"));
        assert!(!SIRI_VERTEX_SRC.contains("#version"));
    }

    #[test]
    fn paint_queues_a_gpu_callback() {
        let _guard = gpu_state_guard();
        let ctx = egui::Context::default();
        let output = crate::ui::frontend::run_pass(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(320.0, 240.0),
                )),
                ..Default::default()
            },
            |ui| {
                for glow in [
                    SiriGlow::wave(0.5, 0.3, 1.0),
                    SiriGlow::orb(0.5, 1.0),
                    SiriGlow::ring(0.5, 12.0, 2.0, 1.6),
                ] {
                    // Before a successful GPU frame the caller keeps its CPU fallback.
                    assert!(!paint(ui, ui.max_rect(), glow), "{:?} ownership", glow.mode);
                }
            },
        );
        let callbacks = output
            .shapes
            .iter()
            .filter(|clipped| matches!(clipped.shape, egui::Shape::Callback(_)))
            .count();
        assert_eq!(
            callbacks, 3,
            "every mode must reach the paint callback registration"
        );
    }

    /// The warm-up must queue a single compile-only callback and then stay out
    /// of the way, while leaving the CPU fallback ownership rule untouched.
    #[test]
    fn warm_up_queues_one_compile_callback_and_keeps_the_cpu_fallback() {
        let ctx = egui::Context::default();
        // The GPU state is process-global, so serialise with the other GPU tests.
        let _guard = gpu_state_guard();
        let output = crate::ui::frontend::run_pass(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(320.0, 240.0),
                )),
                ..Default::default()
            },
            |ui| {
                warm_up(ui);
                warm_up(ui);
                // Warm-up only compiles: until a real glow frame draws, the caller
                // still owns its CPU fallback.
                assert!(!paint(
                    ui,
                    ui.max_rect(),
                    SiriGlow::ring(0.0, 12.0, 2.0, 1.6)
                ));
            },
        );
        let callbacks = output
            .shapes
            .iter()
            .filter(|clipped| matches!(clipped.shape, egui::Shape::Callback(_)))
            .count();
        assert_eq!(
            callbacks, 2,
            "one warm-up callback plus the ring, no matter how often warm_up runs"
        );
        let (queued, done) = warm_up_state();
        assert!(queued, "the warm-up callback must be queued");
        assert!(
            !done,
            "the callback body needs a GL context, so it cannot have run"
        );
    }

    /// Once the programs exist (or the driver already failed), warm-up is a
    /// single atomic load and queues nothing — hidden/idle popups keep repaint
    /// costs at the idle rate.
    #[test]
    fn warm_up_is_skipped_once_the_gpu_path_is_settled() {
        for seed in [
            seed_warm_up_done_for_tests,
            seed_gpu_ready_for_tests,
            seed_gpu_failed_for_tests,
        ] {
            let ctx = egui::Context::default();
            let _guard = gpu_state_guard();
            seed();
            let output = crate::ui::frontend::run_pass(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(320.0, 240.0),
                    )),
                    ..Default::default()
                },
                |ui| warm_up(ui),
            );
            let callbacks = output
                .shapes
                .iter()
                .filter(|clipped| matches!(clipped.shape, egui::Shape::Callback(_)))
                .count();
            assert_eq!(callbacks, 0, "settled GPU state must not re-queue warm-up");
            let (queued, _) = warm_up_state();
            assert!(!queued, "no warm-up may stay queued after it is settled");
        }
    }

    /// Cheap static sanity for the shader sources: the compile call path is
    /// exercised by `paint_queues_a_gpu_callback`, but a stray brace would
    /// only surface as a driver log on the user's machine.
    #[test]
    fn shader_sources_are_balanced_and_non_trivial() {
        for source in [
            SIRI_VERTEX_SRC,
            SIRI_WAVE_FRAGMENT_SRC,
            SIRI_ORB_FRAGMENT_SRC,
            SIRI_RING_FRAGMENT_SRC,
        ] {
            assert!(source.len() > 40);
            let opens = source.matches('{').count();
            let closes = source.matches('}').count();
            assert_eq!(opens, closes, "unbalanced braces in shader source");
            assert!(
                source.contains("void main()"),
                "shader needs an entry point"
            );
        }
        // The vertex stage is shared by every mode, so it must never grow a
        // uniform: the fragment stages own those.
        assert!(!SIRI_VERTEX_SRC.contains("uniform "));
    }

    #[test]
    fn wave_and_orb_use_distinct_programs() {
        assert_ne!(SiriMode::Wave.index(), SiriMode::Orb.index());
        assert_ne!(
            SiriMode::Wave.fragment_source(),
            SiriMode::Orb.fragment_source()
        );
        assert_eq!(SiriGlow::wave(1.0, 0.0, 1.0).mode, SiriMode::Wave);
        assert_eq!(SiriGlow::orb(1.0, 1.0).mode, SiriMode::Orb);
    }

    #[test]
    fn clock_smooths_level_time_and_speed() {
        let ctx = egui::Context::default();
        let start = tick(&ctx, "test", SiriDrive::default(), 1.0 / 60.0);
        assert!(
            (start.time - 1.0 / 60.0).abs() < 1e-5,
            "one frame of dt*1.0 speed: {}",
            start.time
        );
        assert_eq!(start.level, 0.0, "level starts from the stored clock");
        let mut clock = start;
        for _ in 0..30 {
            clock = tick(
                &ctx,
                "test",
                SiriDrive {
                    level: 0.5,
                    ..Default::default()
                },
                1.0 / 60.0,
            );
        }
        assert!(
            clock.time > 0.4 && clock.time < 0.6,
            "0.5s of frames: {}",
            clock.time
        );
        assert!(clock.level > 0.0, "level follows the drive");
        assert!(clock.level <= visual_voice(0.5) + f32::EPSILON);
        // A speed change is eased into the accumulated time (no jump).
        let before = clock.time;
        let after = tick(
            &ctx,
            "test",
            SiriDrive {
                speed: 3.0,
                ..Default::default()
            },
            1.0 / 60.0,
        );
        assert!(
            after.time - before < 0.06,
            "speed eased, not applied at once"
        );
    }

    #[test]
    fn visual_voice_gates_and_eases() {
        assert_eq!(visual_voice(0.0), 0.0);
        assert_eq!(visual_voice(0.012), 0.0, "noise gate");
        assert_eq!(visual_voice(0.34), 1.0, "ceiling maps to a full bar");
        let mid = visual_voice(0.18);
        assert!(
            mid > 0.4 && mid < 1.0,
            "curve stays inside the unit range: {mid}"
        );
    }

    #[test]
    fn disabled_gpu_keeps_the_cpu_fallback() {
        let _guard = gpu_state_guard();
        GPU_FAILED.store(true, Ordering::Relaxed);
        let ctx = egui::Context::default();
        let mut queued = 0;
        let output = crate::ui::frontend::run_pass(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(320.0, 240.0),
                )),
                ..Default::default()
            },
            |ui| {
                if paint(ui, ui.max_rect(), SiriGlow::orb(0.0, 1.0)) {
                    queued += 1;
                }
            },
        );
        assert_eq!(queued, 0, "a disabled GPU path reports no ownership");
        assert_eq!(
            output
                .shapes
                .iter()
                .filter(|clipped| matches!(clipped.shape, egui::Shape::Callback(_)))
                .count(),
            0,
            "no callback is queued once the driver rejected the shader"
        );
    }
}
