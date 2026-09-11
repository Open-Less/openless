import { invokeOrMock } from './shared';
import { APP_VERSION } from '../appVersion';

/** 与 Rust `take_splash_playback` 里 `CARGO_PKG_VERSION` 首段一致的世代标记。 */
export const SPLASH_MAJOR = APP_VERSION.split('.')[0] ?? '0';

const MOCK_SPLASH_MARKER_KEY = 'openless.splashSeenVersion';

/** 进程级判定缓存：StrictMode 双挂载 / 组件重建都复用同一次消费结果，
 *  「是否播放」在一个 webview 进程的生命周期内只向 Rust 问一次。 */
let splashDecision: Promise<boolean> | null = null;

/**
 * 消费「本大版本首启」开屏 PV 标记：true = 本世代首次启动，前端应全屏播放随包
 * 开屏动画一次；false = 配置里已有本世代标记，永远不再播放。真实环境由 Rust 读写
 * preferences.json；浏览器开发模式用 localStorage 模拟同一「播一次后不再播」语义。
 */
export function takeSplashPlayback(): Promise<boolean> {
  splashDecision ??= invokeOrMock<boolean>('take_splash_playback', undefined, () => {
    if (window.localStorage.getItem(MOCK_SPLASH_MARKER_KEY) === SPLASH_MAJOR) {
      return false;
    }
    window.localStorage.setItem(MOCK_SPLASH_MARKER_KEY, SPLASH_MAJOR);
    return true;
  });
  return splashDecision;
}
