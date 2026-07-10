// avatars.tsx — 思考动画与头像。
//
// · ThinkingOrb：与转译胶囊思考态**同一实现**（SiriGL mode="orb"，6 个白色
//   metaball 圆点相互交错转动，thinking speed=1.5 与 Capsule.tsx 一致）。
//   白色光点需要深色底才可见 —— 深色小舞台（olchat-orb-stage）承托，与面板的
//   米白背景形成区分（用户拍板）。WebGL 不可用时退回静态白点环。
// · StaticOrbDots：settled 消息头像用的静态白点环（不占 GL context——长对话里
//   每条消息一个实时 orb 会撞浏览器 WebGL context 上限）。
// · UserAvatar：GitHub 已登录（prefs.marketplaceDevLogin）显示 github.com/
//   {login}.png 头像，未登录 / 加载失败退回 GitHub 图标。仅划词追问（QA）使用。

import { useEffect, useRef, useState } from 'react';
import { ThinkingDots } from '../ThinkingDots';
import { SiriGL, isWebGLAvailable } from '../SiriGL';
import { subscribeOrbFrames } from './orbFeed';
import { getSettings } from '../../lib/ipc/settings';
import { isTauri } from '../../lib/ipc/shared';
import { cn } from './lib/utils';
import './chat.css';

/** 胶囊同款思考动画（深色舞台承托的 SiriGL orb）。 */
export function ThinkingOrb({ size = 56, className }: { size?: number; className?: string }) {
  return (
    <span
      className={cn('olchat-orb-stage', className)}
      style={{ width: size, height: size }}
      aria-hidden
    >
      {isWebGLAvailable() ? (
        <SiriGL mode="orb" speed={1.5} />
      ) : (
        <ThinkingDots
          size={size * 0.5}
          style={{
            position: 'absolute',
            left: '50%',
            top: '50%',
            transform: 'translate(-50%, -50%)',
          }}
        />
      )}
    </span>
  );
}

/**
 * 头像形态的胶囊思考动画：**旋转中的** orb，与胶囊一模一样（用户拍板：
 * 每条助手消息的头像都要转）。不各挂 GL context —— 从共享渲染源
 * （orbFeed，全面板唯一 GL context）逐帧 drawImage 镜像到自己的 2D 小画布。
 * WebGL 不可用时退回 ThinkingDots 圆环。
 */
export function OrbAvatar({ size = 32 }: { size?: number }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [unavailable, setUnavailable] = useState(false);

  useEffect(() => {
    const target = canvasRef.current;
    if (!target) return undefined;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    target.width = Math.round(size * dpr);
    target.height = Math.round(size * dpr);
    const ctx = target.getContext('2d');
    if (!ctx) {
      setUnavailable(true);
      return undefined;
    }
    const unsubscribe = subscribeOrbFrames(source => {
      // 中心裁切放大（源按胶囊构图，光环只占画面 ~1/3；头像里裁掉外圈留白
      // 让转环撑满，动画本体不变）。
      const crop = 0.62;
      const sw = source.width * crop;
      const sh = source.height * crop;
      const sx = (source.width - sw) / 2;
      const sy = (source.height - sh) / 2;
      ctx.clearRect(0, 0, target.width, target.height);
      ctx.drawImage(source, sx, sy, sw, sh, 0, 0, target.width, target.height);
    });
    if (!unsubscribe) {
      setUnavailable(true);
      return undefined;
    }
    return unsubscribe;
  }, [size]);

  if (unavailable) {
    return <ThinkingDots size={size * 0.56} />;
  }
  return (
    <canvas
      ref={canvasRef}
      aria-hidden
      style={{ display: 'block', width: size, height: size, pointerEvents: 'none' }}
    />
  );
}

/**
 * 当前 GitHub 登录名（Marketplace 上传身份，设置里登录后写入 prefs）。
 * refreshKey 变化时重取 —— 面板窗口是常驻 webview（hide/show 复用），
 * 用新会话信号驱动刷新即可跟上登录状态变化。
 */
export function useGithubLogin(refreshKey?: string | number): string {
  const [login, setLogin] = useState('');
  useEffect(() => {
    if (!isTauri) return;
    let cancelled = false;
    getSettings()
      .then(prefs => {
        if (!cancelled) setLogin((prefs.marketplaceDevLogin ?? '').trim());
      })
      .catch(() => {
        /* 未登录 / 读取失败：保持图标兜底。 */
      });
    return () => {
      cancelled = true;
    };
  }, [refreshKey]);
  return login;
}

/** 用户头像（QA 专用）：GitHub 头像图 / 未登录 GitHub 图标。 */
export function UserAvatar({ login }: { login: string }) {
  const [broken, setBroken] = useState(false);
  useEffect(() => setBroken(false), [login]);
  const showImage = login.length > 0 && !broken;
  return showImage ? (
    <img
      className="size-8 rounded-full object-cover"
      src={`https://github.com/${encodeURIComponent(login)}.png?size=64`}
      alt=""
      loading="lazy"
      draggable={false}
      onError={() => setBroken(true)}
    />
  ) : (
    <span className="flex size-8 items-center justify-center text-foreground/70">
      <GithubMark />
    </span>
  );
}

function GithubMark() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor" aria-hidden>
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
    </svg>
  );
}
