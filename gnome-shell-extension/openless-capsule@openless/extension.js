// OpenLess Capsule — GNOME Shell 扩展（GNOME 49/50, ESM）
// 语音输入悬浮胶囊：监听 OpenLess 状态桥（Unix socket JSON lines），
// 在屏幕底部居中渲染 Siri 流体光球。
// - 录音/忙碌态：GLSL 着色器（metaball 流体 + 光谱色带 + 色差 + 辉光，音量驱动）
// - 终态（完成/取消/出错）：Cairo 图标（符号锐利）
// - 着色器不可用时整体回退 Cairo
// 置顶、鼠标穿透、不抢焦点；OpenLess 不在时零存在感。
import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import GObject from 'gi://GObject';
import St from 'gi://St';
import Clutter from 'gi://Clutter';
import Cairo from 'gi://cairo';
import Shell from 'gi://Shell';
import Meta from 'gi://Meta';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

// 画布只罩住球体+光晕（无底板）；桥正常时 level 帧约 30Hz，超时视为失联
const W = 220, H = 150, BOTTOM_MARGIN = 70, REPAINT_MS = 16, TERMINAL_MS = 2000,
    WATCHDOG_SEC = 5;
const SOCKET_PATH = `${GLib.get_user_cache_dir()}/openless/capsule-state.sock`;
// 着色器热替换：此文件存在时以其函数体（void main...）覆盖内置着色器，
// 修改后 ~2 秒内生效（免注销调参通道）。删除文件即回到内置版本。
const FRAG_OVERRIDE_PATH = `${GLib.get_user_cache_dir()}/openless/capsule-shader.frag`;
const DEMO = GLib.getenv('OPENLESS_CAPSULE_DEMO') === '1';
const NO_SHADER = GLib.getenv('OPENLESS_CAPSULE_NO_SHADER') === '1';

const RGBA = (cr, r, g, b, a) => cr.setSourceRGBA(r, g, b, a);

// 深色柔光衬底：浅色桌面下保证对比度（Cairo 回退路径用）
function softBacking(cr, cx, cy, radius, alpha) {
    const g = new Cairo.RadialGradient(cx, cy, radius * 0.35, cx, cy, radius);
    g.addColorStopRGBA(0, 0.06, 0.06, 0.09, alpha);
    g.addColorStopRGBA(0.75, 0.06, 0.06, 0.09, alpha * 0.55);
    g.addColorStopRGBA(1, 0.06, 0.06, 0.09, 0);
    cr.setSource(g);
    cr.arc(cx, cy, radius, 0, Math.PI * 2);
    cr.fill();
}

// ———— GLSL：Siri 流体光球（metaball + smin 软融合 + 光谱色带 + 色差）————
// 自包含、无纹理依赖；u_volume/u_peak 驱动半径与亮度，u_busy 区分录音/处理态
const SHADER_DECLS = `
uniform float u_time;
uniform float u_volume;
uniform float u_peak;
uniform float u_busy;   // 0=recording 1=busy(转写/润色)
uniform float u_w;
uniform float u_h;
const float TAU = 6.28318530718;
float hash11(float n){ return fract(sin(n*127.1 + 311.7)*43758.5453); }
vec3 hue2rgb(float h){
    h = fract(h);
    float r = clamp(abs(h*6.0 - 3.0) - 1.0, 0.0, 1.0);
    float g = clamp(2.0 - abs(h*6.0 - 2.0), 0.0, 1.0);
    float b = clamp(2.0 - abs(h*6.0 - 4.0), 0.0, 1.0);
    return vec3(r, g, b);
}
float smin(float a, float b, float k){
    float h = max(k - abs(a - b), 0.0) / k;
    return min(a, b) - h*h*k*0.25;
}
// 6 个 metaball 沿椭圆轨道运行；vol/pk 驱动半径与亮度，说话时形体涨大
float orbField(vec2 p, float t, float vol, float pk){
    float d = 1e5;
    for (int i = 0; i < 6; i++) {
        float fi = float(i);
        float h1 = hash11(fi*3.7 + 1.3);
        float h2 = hash11(fi*7.1 + 4.9);
        float ang = t*(0.55 + h1*0.85) + fi*1.047 + h2*6.28;
        float rx = 0.34 + 0.10*sin(t*0.7 + fi*2.1);
        float ry = 0.17 + 0.05*cos(t*0.9 + fi*1.3);
        vec2 c = vec2(cos(ang)*rx, sin(ang)*ry);
        float rr = (0.075 + 0.05*h1) * (0.45 + 0.95*vol + 0.45*pk*h2);
        d = smin(d, length(p - c) - rr, 0.085);
    }
    return d;
}
// 光谱色带：核心亮蓝 → 中层青蓝 → 外圈紫/粉（避开绿色段）
vec3 orbColor(float x, float t){
    vec3 core = vec3(0.72, 0.87, 1.0);
    vec3 mid  = hue2rgb(fract(0.58 + 0.04*sin(t*0.5)));
    vec3 rim  = hue2rgb(fract(0.90 - 0.16*x + 0.05*sin(t*0.35)));
    return mix(mix(core, mid, smoothstep(0.05, 0.45, x)), rim, smoothstep(0.4, 0.95, x));
}
`;

const SHADER_BODY = `
// ===== Waveform Ring —— 移植自 VoiceOrbs waveform-ring (MIT, © Alexis Munoz)
// 极坐标波形环：合成谐波（官方回退式）+ 整流偏置；level 驱动振幅/线宽/中心辉光
void main() {
    vec2 uv = cogl_tex_coord_in[0].xy;
    vec2 p = (uv - 0.5) * 2.0 * vec2(u_w / u_h, 1.0);
    float t = u_time;
    float vol = clamp(u_volume, 0.0, 1.0);
    float pk = clamp(u_peak, 0.0, 1.0);

    float ang = atan(p.y, p.x);
    float r = length(p);

    // 合成波形（VoiceOrbs listening 回退式）+ 说话时高频细节档
    float shape = 0.62 * sin(6.0 * ang + 7.2 * t)
                + 0.38 * sin(11.0 * ang - 9.6 * t);
    shape = mix(shape, abs(shape), 0.85);              // 整流：只往外凸
    shape += (0.30 + 0.7 * vol) * 0.42 * sin(17.0 * ang + 4.4 * t);

    // 振幅主通道：level 驱动起伏，静音时收敛为纯净圆环
    float amp = 0.22 * (0.35 + 0.65 * vol) + 0.07 * pk;
    float R0 = 0.32;
    float rr = R0 * (1.0 + amp * shape + 0.05 * vol);

    // 双层描边：宽辉光 + 细主线（线宽随音量）
    float d = abs(r - rr);
    float lineWidth = 0.012 + 0.010 * vol;
    float core = exp(-(d * d) / (lineWidth * lineWidth));
    float glow = exp(-d / (lineWidth * 3.2)) * 0.6;

    // 颜色：沿角度蓝→紫→青流转
    vec3 cA = vec3(0.506, 0.549, 0.973);
    vec3 cB = vec3(0.655, 0.545, 0.980);
    vec3 cC = vec3(0.133, 0.827, 0.933);
    float cm = 0.5 + 0.5 * sin(ang * 2.0 + t * 0.7);
    vec3 col = mix(cA, cB, cm);
    col = mix(col, cC, 0.25 + 0.25 * vol);

    vec3 outc = col * (core * 1.1 + glow * 0.8);
    outc += col * exp(-r * r * 24.0) * (0.10 + 0.25 * vol);   // 中心辉光

    // 范围保险 + 输出
    float alpha = clamp(max(max(outc.r, outc.g), outc.b), 0.0, 1.0);
    alpha *= 1.0 - smoothstep(0.46, 0.52, r);
    cogl_color_out = vec4(outc, alpha);
}
`;

// ———— GLSL 效果封装（legacy set_shader_source 路线；paint_target 上传 uniform）————
const SiriShaderEffect = GObject.registerClass(
class SiriShaderEffect extends Clutter.ShaderEffect {
    _init(host) {
        super._init({shader_type: 1});   // FRAGMENT=1；mutter 50 从 GI 移除了 ShaderType 枚举
        this._host = host;
        this.set_shader_source(SHADER_DECLS + SHADER_BODY);
    }

    vfunc_paint_target(...args) {
        if (this._host) {
            this._upload('u_time', this._host.shaderTime);
            this._upload('u_volume', this._host.envVol);
            this._upload('u_peak', this._host.envPeak);
            const busy = (this._host.mode === 'transcribing' ||
                          this._host.mode === 'polishing') ? 1 : 0;
            this._upload('u_busy', busy);
            this._upload('u_w', W);
            this._upload('u_h', H);
        }
        super.vfunc_paint_target(...args);
    }

    _upload(name, value) {
        const isInt = name === 'u_busy';
        const val = new GObject.Value();
        if (isInt) {
            val.init(GObject.TYPE_INT);
            val.set_int(Math.trunc(value ?? 0));
        } else {
            val.init(GObject.TYPE_FLOAT);
            val.set_float(parseFloat(value ?? 0));
        }
        try {
            this.set_uniform_value(name, val);
        } catch (e) { /* 单个 uniform 失败不致命 */ }
    }
});

const CapsuleArea = GObject.registerClass(
class CapsuleArea extends St.DrawingArea {
    _init() {
        super._init({reactive: false, style_class: 'openless-capsule'});
        this.set_width(W);
        this.set_height(H);
        this._state = 'idle';
        this._level = 0;
        this._shownAt = 0;
        this._t0 = GLib.get_monotonic_time() / 1e6;
        this.shaderActive = false;   // 录音/忙碌态由着色器接管
        this.connect('repaint', this._onDraw.bind(this));
    }

    setState(state, level = 0) {
        const prev = this._state;
        this._state = state;
        this._level = level;
        if (state !== 'idle' && (prev === 'idle' || prev === undefined))
            this._shownAt = GLib.get_monotonic_time() / 1e6;
        // 空闲彻底隐藏（零存在感），收到新状态立即恢复
        if (state === 'idle')
            this.hide();
        else
            this.show();
        this.queue_repaint();
    }

    get visibleState() {
        return this._state;
    }

    _onDraw(area) {
        const cr = area.get_context();
        const now = GLib.get_monotonic_time() / 1e6 - this._t0;
        cr.setOperator(Cairo.Operator.CLEAR);
        cr.paint();
        cr.setOperator(Cairo.Operator.OVER);

        // 录音/忙碌态由 GLSL 接管（effect 已挂在 actor 上），cairo 留空
        if (this.shaderActive &&
            (this._state === 'recording' || this._state === 'transcribing' ||
             this._state === 'polishing'))
            return Clutter.EVENT_PROPAGATE;

        const cx = W / 2, cy = H / 2;
        const t = now;
        switch (this._state) {
        case 'recording': {
            // 呼吸光球：半径、抖动、光晕全部跟随音量——说话时动，静音完全静止
            const lvl = Math.min(1, Math.max(0, this._level));
            const r = 13 + lvl * 11 + lvl * Math.sin(t * 5) * 2.5;
            softBacking(cr, cx, cy, r * 2.4, 0.28 + 0.17 * lvl);
            const glowR = r * 2.2;
            const grad = new Cairo.RadialGradient(cx, cy, r * 0.2, cx, cy, glowR);
            grad.addColorStopRGBA(0, 0.45, 0.72, 1.0, 0.55 * (0.3 + 0.7 * lvl));
            grad.addColorStopRGBA(1, 0.30, 0.55, 1.0, 0.0);
            cr.setSource(grad);
            cr.arc(cx, cy, glowR, 0, Math.PI * 2);
            cr.fill();
            RGBA(cr, 0.62, 0.83, 1.0, 0.95);
            cr.arc(cx, cy, r, 0, Math.PI * 2);
            cr.fill();
            break;
        }
        case 'transcribing':
        case 'polishing': {
            // 旋转圆环
            const polishing = this._state === 'polishing';
            const col = polishing ? [0.75, 0.55, 1.0] : [0.45, 0.75, 1.0];
            const r = 16 + Math.sin(t * 3) * 1.6;
            softBacking(cr, cx, cy, r * 2.1, 0.38);
            for (let i = 0; i < 3; i++) {
                const a0 = t * 3.2 + (i * Math.PI * 2) / 3;
                RGBA(cr, col[0], col[1], col[2], 0.9 - i * 0.28);
                cr.setLineWidth(4 - i);
                cr.arc(cx, cy, r - i * 4, a0, a0 + 1.1);
                cr.stroke();
            }
            break;
        }
        case 'done': {
            softBacking(cr, cx, cy, 30, 0.38);
            RGBA(cr, 0.35, 0.9, 0.55, 0.95);
            cr.arc(cx, cy, 13, 0, Math.PI * 2);
            cr.fill();
            RGBA(cr, 0.06, 0.2, 0.1, 1);
            cr.setLineWidth(3);
            cr.moveTo(cx - 5.5, cy);
            cr.lineTo(cx - 1.5, cy + 5);
            cr.lineTo(cx + 6.5, cy - 5);
            cr.stroke();
            break;
        }
        case 'cancelled': {
            softBacking(cr, cx, cy, 30, 0.38);
            RGBA(cr, 0.55, 0.55, 0.6, 0.8);
            cr.setLineWidth(3.5);
            cr.moveTo(cx - 6.5, cy - 6.5);
            cr.lineTo(cx + 6.5, cy + 6.5);
            cr.moveTo(cx + 6.5, cy - 6.5);
            cr.lineTo(cx - 6.5, cy + 6.5);
            cr.stroke();
            break;
        }
        case 'error': {
            softBacking(cr, cx, cy, 30, 0.38);
            RGBA(cr, 0.95, 0.35, 0.35, 0.95);
            cr.arc(cx, cy, 13 + Math.sin(t * 8) * 1.2, 0, Math.PI * 2);
            cr.fill();
            break;
        }
        default:
            break;
        }
        return Clutter.EVENT_PROPAGATE;
    }
});

export default class OpenLessCapsuleExtension extends Extension {
    enable() {
        this._area = new CapsuleArea();
        this._area.reactive = false;
        this._conn = null;
        this._cancellable = null;
        this._buffer = '';
        this._reconnectSource = null;
        this._animSource = null;
        this._demoSource = null;
        this._demoStep = 0;
        this._idleSource = null;

        // 包络 / 着色器驱动状态（字符串状态体系）
        this.mode = 'idle';
        this.shaderTime = 0;
        this.envVol = 0;
        this.envPeak = 0;
        this.lastLevel = 0;
        this._lastFrameAt = 0;
        this._lastFrameMono = 0;

        Main.layoutManager.addTopChrome(this._area);
        this._area.hide();  // 初始零存在感，收到第一帧状态再显示
        this._place();
        this._monitorsChanged = Main.layoutManager.connect(
            'monitors-changed', () => this._place());

        // 着色器优先（录音/忙碌态），失败回退 Cairo
        this._shaderWanted = !NO_SHADER && typeof Clutter.ShaderEffect !== 'undefined';
        this._shaderAttached = false;
        if (this._shaderWanted) {
            try {
                this._effect = new SiriShaderEffect(this);
                console.log('[openless-capsule] GLSL Siri orb effect created');
                this._startFragWatch();
            } catch (e) {
                console.warn('[openless-capsule] shader create failed, cairo fallback:', e);
                this._effect = null;
                this._shaderWanted = false;
            }
        }

        // 全局听写热键：Mutter 抓取，与焦点应用无关。
        // （修复：fcitx5 无 Wayland 原生 IM 协议，插件热键依赖焦点应用以 fcitx 模块
        //   启动；全局抓取让任意窗口下右 Alt 都触发，且被合成器消费不双触发。）
        try {
            const schemaSource = Gio.SettingsSchemaSource.new_from_directory(
                GLib.build_filenamev([this.path, 'schemas']),
                Gio.SettingsSchemaSource.get_default(), false);
            const schema = schemaSource.lookup('org.gnome.shell.extensions.openless-capsule', false);
            if (schema) {
                this._settings = new Gio.Settings({settings_schema: schema});
                Main.wm.addKeybinding('openless-dictation', this._settings,
                    Meta.KeyBindingFlags.NONE, Shell.ActionMode.ALL,
                    () => this._toggleDictation());
                console.log('[openless-capsule] global dictation hotkey registered');
            } else {
                console.warn('[openless-capsule] schema not found; fcitx plugin hotkey remains');
            }
        } catch (e) {
            console.warn('[openless-capsule] keybinding failed (fcitx plugin hotkey still works):', e);
        }

        // 60fps 驱动：包络 + uniform 上传（仅在可见时渲染）
        this._animSource = GLib.timeout_add(GLib.PRIORITY_DEFAULT, REPAINT_MS, () => {
            const now = GLib.get_monotonic_time() / 1e6;
            const st = this._area.visibleState;
            if (st === 'idle')
                return GLib.SOURCE_CONTINUE;

            const dt = this._lastFrameMono > 0
                ? Math.min(0.2, Math.max(0.001, now - this._lastFrameMono)) : 0.016;
            this._lastFrameMono = now;
            this.shaderTime += dt * (st === 'recording' ? 1.0 : 0.55);
            // 包络：应用侧已做噪声门/归一化/包络，这里轻度平滑 + 峰值保持
            const k = this.lastLevel > this.envVol
                ? 1 - Math.exp(-dt / 0.045)
                : 1 - Math.exp(-dt / 0.22);
            this.envVol += (this.lastLevel - this.envVol) * k;
            this.envPeak = Math.max(this.lastLevel, this.envPeak * Math.exp(-dt / 0.15));

            // 看门狗：桥失联/卡死时收回胶囊，避免永久悬浮挡内容
            if (this._lastFrameAt > 0 &&
                now - this._area._shownAt > WATCHDOG_SEC &&
                now - this._lastFrameAt > WATCHDOG_SEC) {
                this._area.setState('idle');
                return GLib.SOURCE_CONTINUE;
            }

            // effect 挂/摘跟随状态（终态图标走 Cairo，符号更锐利）
            if (this._shaderWanted && this._effect) {
                const wantShader = st === 'recording' || st === 'transcribing' ||
                    st === 'polishing';
                if (wantShader && !this._shaderAttached) {
                    this._area.add_effect_with_name('siri-orb', this._effect);
                    this._shaderAttached = true;
                    this._area.shaderActive = true;
                } else if (!wantShader && this._shaderAttached) {
                    this._area.remove_effect_by_name('siri-orb');
                    this._shaderAttached = false;
                    this._area.shaderActive = false;
                }
            }

            this._area.queue_repaint();
            return GLib.SOURCE_CONTINUE;
        });

        if (DEMO) {
            this._startDemo();
        } else {
            this._connectSocket();
        }
    }

    _place() {
        const m = Main.layoutManager.primaryMonitor;
        if (!m)
            return;
        this._area.set_position(
            m.x + Math.floor((m.width - W) / 2),
            m.y + m.height - H - BOTTOM_MARGIN);
    }

    // —— 真数据：Unix socket JSON lines ——
    _connectSocket() {
        if (this._conn || !this._area)
            return;
        this._cancellable = new Gio.Cancellable();
        const client = new Gio.SocketClient();
        const addr = new Gio.UnixSocketAddress({ path: SOCKET_PATH });
        client.connect_async(addr, this._cancellable, (c, res) => {
            try {
                this._conn = c.connect_finish(res);
                this._buffer = '';
                this._readLine();
            } catch (e) {
                this._conn = null;
                this._scheduleReconnect();
            }
        });
    }

    _scheduleReconnect() {
        if (this._reconnectSource || DEMO || !this._area)
            return;
        this._reconnectSource = GLib.timeout_add_seconds(
            GLib.PRIORITY_DEFAULT, 2, () => {
                this._reconnectSource = null;
                this._connectSocket();
                return GLib.SOURCE_REMOVE;
            });
    }

    _readLine() {
        if (!this._conn)
            return;
        this._conn.get_input_stream().read_bytes_async(
            4096, GLib.PRIORITY_DEFAULT, this._cancellable, (s, res) => {
                try {
                    const bytes = s.read_bytes_finish(res);
                    if (bytes.get_size() === 0)
                        throw new Error('EOF');
                    const text = new TextDecoder().decode(bytes.get_data());
                    this._buffer += text;
                    let idx;
                    while ((idx = this._buffer.indexOf('\n')) >= 0) {
                        const line = this._buffer.slice(0, idx).trim();
                        this._buffer = this._buffer.slice(idx + 1);
                        if (!line)
                            continue;
                        try {
                            if (!this._area)
                                return;
                            this._lastFrameAt = GLib.get_monotonic_time() / 1e6;
                            const msg = JSON.parse(line);
                            this.lastLevel = msg.level ?? 0;
                            this._area.setState(msg.state ?? 'idle', msg.level ?? 0);
                            if (this._isTerminal(msg.state))
                                this._scheduleIdle(msg.state);
                        } catch (e) { /* 坏行忽略 */ }
                    }
                    this._readLine();
                } catch (e) {
                    this._teardownConn();
                    if (this._area)
                        this._area.setState('idle');
                    this._scheduleReconnect();
                }
            });
    }

    _isTerminal(state) {
        return state === 'done' || state === 'cancelled' || state === 'error';
    }

    // 终态停留 TERMINAL_MS 后收回 idle（扩展隐藏）
    _scheduleIdle(_state) {
        if (this._idleSource)
            GLib.source_remove(this._idleSource);
        this._idleSource = GLib.timeout_add(GLib.PRIORITY_DEFAULT, TERMINAL_MS, () => {
            this._idleSource = null;
            this._area.setState('idle');
            return GLib.SOURCE_REMOVE;
        });
    }

    _teardownConn() {
        if (this._cancellable)
            this._cancellable.cancel();
        if (this._conn) {
            try { this._conn.close(null); } catch (e) { /* 已断 */ }
            this._conn = null;
        }
    }

    // —— 假数据：嵌套 shell / 无 OpenLess 环境的可视化验证 ——
    _startDemo() {
        const steps = [
            ['recording', 5],
            ['transcribing', 2],
            ['polishing', 1.5],
            ['done', 2],
            ['idle', 2],
        ];
        const stepFn = () => {
            const [state, secs] = steps[this._demoStep % steps.length];
            this._demoStep += 1;
            this._area.setState(state, state === 'recording' ? 0.5 : 0);
            if (state === 'recording')
                this._demoLevel();
            this._demoSource = GLib.timeout_add(
                GLib.PRIORITY_DEFAULT, secs * 1000, stepFn);
            return GLib.SOURCE_REMOVE;
        };
        this._demoSource = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 800, stepFn);
    }

    _demoLevel() {
        const t0 = GLib.get_monotonic_time() / 1e6;
        const push = () => {
            if (this._area.visibleState !== 'recording')
                return GLib.SOURCE_REMOVE;
            const t = GLib.get_monotonic_time() / 1e6 - t0;
            this._area.setState('recording', 0.35 + 0.35 * Math.abs(Math.sin(t * 2.4)));
            return GLib.SOURCE_CONTINUE;
        };
        GLib.timeout_add(GLib.PRIORITY_DEFAULT, 90, push);
    }

    // 着色器热替换轮询：frag 文件变化 → 重建 effect（移除+重挂强制重新编译）
    _startFragWatch() {
        if (DEMO)
            return;
        this._fragBody = null;
        this._fragPoll = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 2, () => {
            try {
                const [ok, bytes] = GLib.file_get_contents(FRAG_OVERRIDE_PATH);
                if (!ok)
                    return GLib.SOURCE_CONTINUE;
                const body = new TextDecoder().decode(bytes);
                if (body !== this._fragBody && body.includes('void main')) {
                    this._fragBody = body;
                    // 关键：必须新建 effect 实例——在旧实例上 set_shader_source 不会
                    // 触发 Cogl 重编译（管线缓存），必须新对象才拿得到新着色器。
                    if (this._effect) {
                        try { this._area.remove_effect_by_name('siri-orb'); } catch (e) { /* */ }
                        this._effect = null;
                    }
                    try {
                        this._effect = new SiriShaderEffect(this);
                        this._effect.set_shader_source(SHADER_DECLS + body);
                        this._area.add_effect_with_name('siri-orb', this._effect);
                        this._shaderAttached = this._shaderWanted;
                        this._area.shaderActive = this._shaderWanted &&
                            (this._area.visibleState === 'recording' ||
                             this._area.visibleState === 'transcribing' ||
                             this._area.visibleState === 'polishing');
                        console.log('[openless-capsule] shader hot-swapped from frag file');
                    } catch (e) {
                        console.warn('[openless-capsule] frag swap failed:', e);
                    }
                }
            } catch (e) { /* 读失败忽略 */ }
            return GLib.SOURCE_CONTINUE;
        });
    }

    // 全局热键处理器：向 OpenLess 发听写键事件（应用 Toggle 语义：按一下切换）
    _toggleDictation() {
        try {
            if (!this._dbusConn)
                this._dbusConn = Gio.bus_get_sync(Gio.BusType.SESSION, null);
            this._dbusConn.emit_signal(null, '/openless',
                'org.fcitx.Fcitx.OpenLess1', 'DictationKeyEvent',
                new GLib.Variant('(uub)', [65514, 108, true]));
            return Clutter.EVENT_STOP;
        } catch (e) {
            console.warn('[openless-capsule] toggle emit failed:', e);
            return Clutter.EVENT_PROPAGATE;
        }
    }

    disable() {
        if (this._settings) {
            try { Main.wm.removeKeybinding('openless-dictation'); } catch (e) { /* 已移除 */ }
            this._settings = null;
        }
        if (this._dbusConn) {
            try { this._dbusConn.close(null); } catch (e) { /* 已断 */ }
            this._dbusConn = null;
        }
        if (this._fragPoll) { GLib.source_remove(this._fragPoll); this._fragPoll = null; }
        if (this._idleSource) { GLib.source_remove(this._idleSource); this._idleSource = null; }
        if (this._reconnectSource) { GLib.source_remove(this._reconnectSource); this._reconnectSource = null; }
        if (this._animSource) { GLib.source_remove(this._animSource); this._animSource = null; }
        if (this._demoSource) { GLib.source_remove(this._demoSource); this._demoSource = null; }
        this._teardownConn();
        if (this._monitorsChanged) {
            Main.layoutManager.disconnect(this._monitorsChanged);
            this._monitorsChanged = null;
        }
        if (this._effect && this._area)
            this._area.remove_effect(this._effect);
        Main.layoutManager.removeChrome(this._area);
        this._area.destroy();
        this._area = null;
    }
}
