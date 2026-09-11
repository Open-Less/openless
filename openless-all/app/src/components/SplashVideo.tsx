import { useEffect, useRef, useState } from 'react';
import { takeSplashPlayback } from '../lib/ipc';

/** 随包发行的 2.0 开屏 PV（public/ 静态资源，Vite 原样打进 dist）。 */
const SPLASH_SRC = '/openless-2.0-splash.mp4';
/** 播放结束后整层渐隐的时长，与 global.css 的 ol-splash-out 保持一致。 */
const FADE_MS = 1200;
/** 看门狗：`ended` / `error` 双双失灵（损坏编码、后台节流挂起）时，
 *  开屏层最多占用 25s 也必须渐隐让位，绝不把用户永久挡在动画后面。 */
const SPLASH_WATCHDOG_MS = 25_000;

type SplashPhase = 'pending' | 'playing' | 'fading' | 'done';

/**
 * 2.0 开屏 PV：仅在「配置文件里没有本大版本标记」的首启播放一次。
 * 标记由 Rust `take_splash_playback` 读写 preferences.json（浏览器开发模式用
 * localStorage 同语义模拟），播放判定发生在组件挂载时，因此每个 webview
 * 进程只会消费一次。
 *
 * 表现要求：全屏铺满（任意窗口比例下 object-fit: cover 裁切填满，无黑边）；
 * 播完从最后一帧画面开始整体渐隐到透明消失（ol-splash-out）。
 */
export function SplashVideo() {
  const [phase, setPhase] = useState<SplashPhase>('pending');
  const videoRef = useRef<HTMLVideoElement | null>(null);

  useEffect(() => {
    let cancelled = false;
    takeSplashPlayback()
      .then((shouldPlay) => {
        if (!cancelled) setPhase(shouldPlay ? 'playing' : 'done');
      })
      .catch(() => {
        // 判定 IPC 失败 = 不播。开屏动画是锦上添花，绝不因它挡住应用本身。
        if (!cancelled) setPhase('done');
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (phase !== 'playing') return;
    const watchdog = window.setTimeout(() => setPhase('fading'), SPLASH_WATCHDOG_MS);
    return () => window.clearTimeout(watchdog);
  }, [phase]);

  useEffect(() => {
    if (phase !== 'fading') return;
    const timer = window.setTimeout(() => setPhase('done'), FADE_MS);
    return () => window.clearTimeout(timer);
  }, [phase]);

  useEffect(() => {
    if (phase !== 'playing') return;
    const video = videoRef.current;
    if (!video) return;
    // autoPlay 属性先尝试有声播放；WKWebView 拒绝带音频的自动播放时降级为
    // 静音续播——保证动画画面永远完整，声音能出则出。
    video.play().catch(() => {
      video.muted = true;
      video.play().catch(() => setPhase('fading'));
    });
  }, [phase]);

  if (phase === 'pending' || phase === 'done') return null;
  return (
    <div
      className="ol-splash"
      data-fading={phase === 'fading' ? 'true' : undefined}
      role="presentation"
    >
      <video
        ref={videoRef}
        className="ol-splash-video"
        src={SPLASH_SRC}
        autoPlay
        playsInline
        onEnded={() => setPhase('fading')}
        onError={() => setPhase('fading')}
      />
    </div>
  );
}
