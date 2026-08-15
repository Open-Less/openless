// 「这段话没能落进去」的兜底卡片，弹在屏幕右下角。
//
// 为什么需要它：文本没能插进目标 app 时（上屏途中你切走了窗口、密码框挡着、粘贴被拒），
// 此前唯一的兜底是**悄悄**把文本写进剪贴板。那有两个问题——它依赖一个默认可关的开关，
// 而且就算开着，屏幕上也没有任何东西告诉你「你刚说的那段话在剪贴板里」。你看到的只是
// 胶囊一闪而过，然后是空的输入框，或者被守卫截断的半截话。
//
// 所以卡片必须把**完整**的那段话摆出来：切走窗口的人要的是整段，不是屏幕上残留的半截。
//
// 为什么复制走后端：卡片浮在别的 app 上面，按钮刻意 preventDefault 不抢焦点（抢了就把
// 你正在写的地方的光标弄没了），而未聚焦的文档调 navigator.clipboard 会直接抛
// `Document is not focused`。
//
// 为什么 TTL 比词条卡片长一倍：那张卡片只要瞄一眼「记不记这个词」，这张要把一段话读完
// 再决定复不复制。悬停时还会停表——正在读的时候卡片消失是最气人的。
//
// 为什么没有标题：卡片突然出现、里面是你刚说的那段话、下面一个「复制」——这三件事凑在
// 一起，意思已经到了。原先那行「你切走了窗口，这段话没能落进去」是在替用户解释他自己
// 刚做过的动作，读起来像旁白。`payload.reason` 仍然保留，但只进日志，不上屏。

import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  copyTextToClipboard,
  dismissInsertFallbackCard,
  reportInsertFallbackCardHeight,
} from '../lib/ipc';
import {
  nextFallbackCardHeightReport,
  type FallbackCardHeightReport,
} from '../lib/insertFallbackLayout';
import type { InsertFallbackCardPayload } from '../lib/types';

/// 卡片自己消失的时间。比词条卡片的 10 秒长——这张要读内容。
const TTL_MS = 20_000;
/// 复制成功后按钮停留在「已复制」的时间。
const COPIED_FEEDBACK_MS = 1_600;
/// 正文最多显示几行，再多就在卡片内部滚动。
/// 原生窗口使用 DOM 实测高度，不再重复维护这组布局参数。
const MAX_LINES = 8;
const LINE_HEIGHT = 18;

interface InsertFallbackCardProps {
  payload: InsertFallbackCardPayload;
}

export function InsertFallbackCard({ payload }: InsertFallbackCardProps) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);
  const [paused, setPaused] = useState(false);
  const timerRef = useRef<number | null>(null);
  const cardRootRef = useRef<HTMLDivElement | null>(null);
  const lastHeightReportRef = useRef<FallbackCardHeightReport | null>(null);

  useLayoutEffect(() => {
    const element = cardRootRef.current;
    if (!element) return undefined;
    let cancelled = false;

    const reportHeight = () => {
      const report = nextFallbackCardHeightReport(
        lastHeightReportRef.current,
        payload.presentationId,
        element.getBoundingClientRect().height,
      );
      if (!report) return;
      lastHeightReportRef.current = report;
      void reportInsertFallbackCardHeight(report.presentationId, report.height).catch(() => {
        // IPC 短暂失败后允许后续 ResizeObserver 通知重试。
        if (
          !cancelled
          && lastHeightReportRef.current?.presentationId === report.presentationId
          && lastHeightReportRef.current.height === report.height
        ) {
          lastHeightReportRef.current = null;
        }
      });
    };

    lastHeightReportRef.current = null;
    reportHeight();
    if (typeof ResizeObserver === 'undefined') return undefined;
    const observer = new ResizeObserver(reportHeight);
    observer.observe(element);
    return () => {
      cancelled = true;
      observer.disconnect();
    };
  }, [payload.presentationId]);

  // TTL 倒计时。悬停时暂停：鼠标停在卡片上说明人正在读它。
  useEffect(() => {
    if (paused) return;
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = window.setTimeout(() => {
      void dismissInsertFallbackCard();
    }, TTL_MS);
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, [paused, payload.text]);

  const copy = async () => {
    try {
      await copyTextToClipboard(payload.text);
      setCopied(true);
      setCopyFailed(false);
      window.setTimeout(() => setCopied(false), COPIED_FEEDBACK_MS);
    } catch {
      // 复制失败要说出来——这张卡片本身就是「文本别丢了」的最后一道保障，
      // 再静默失败一次，用户就真的没有任何途径拿到这段话了。
      setCopyFailed(true);
    }
  };

  return (
    <div
      ref={cardRootRef}
      style={{
        width: '100%',
        alignSelf: 'flex-end',
        display: 'flex',
        flexDirection: 'column',
        justifyContent: 'flex-end',
        padding: 12,
        // 卡片是唯一要接鼠标的东西——胶囊本体全程 pointerEvents:none。
        pointerEvents: 'auto',
        boxSizing: 'border-box',
      }}
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
    >
      <div
        style={{
          borderRadius: 16,
          padding: 12,
          background: 'var(--ol-capsule-pill-bg)',
          backdropFilter: 'blur(20px)',
          WebkitBackdropFilter: 'blur(20px)',
          border: '1px solid var(--ol-capsule-pill-border)',
          boxShadow:
            'var(--ol-capsule-pill-shadow), var(--ol-capsule-pill-inset)',
          color: 'var(--ol-capsule-btn-ink)',
          fontFamily: 'var(--ol-font-sans)',
          overflow: 'hidden',
          animation: 'capsule-in .28s cubic-bezier(.3,1.1,.4,1) both',
        }}
      >
        <div
          style={{
            fontSize: 13,
            lineHeight: `${LINE_HEIGHT}px`,
            maxHeight: LINE_HEIGHT * MAX_LINES,
            overflowY: 'auto',
            // 让用户能手动选一段——有人只想要其中一句。
            userSelect: 'text',
            cursor: 'text',
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            marginBottom: 10,
          }}
        >
          {payload.text}
        </div>

        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <TextButton
            primary
            label={
              copyFailed
                ? t('insertFallbackCard.copyFailed')
                : copied
                  ? t('insertFallbackCard.copied')
                  : t('insertFallbackCard.copy')
            }
            onClick={() => void copy()}
          />
          <TextButton
            label={t('insertFallbackCard.dismiss')}
            onClick={() => void dismissInsertFallbackCard()}
          />
        </div>
      </div>
    </div>
  );
}

/// 按钮配色与胶囊上那对确认/取消同源；这里是带文字的宽按钮，因为「复制」需要说清楚
/// 自己干了什么，一个图标撑不住。
function TextButton({
  label,
  primary,
  onClick,
}: {
  label: string;
  primary?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      // 卡片浮在别的 app 上面，按下去不能把焦点从用户正在写的地方抢走。
      // 这也正是复制必须走后端的原因（navigator.clipboard 要求文档聚焦）。
      onMouseDown={event => {
        event.preventDefault();
        event.stopPropagation();
      }}
      style={{
        flex: primary ? 1 : undefined,
        height: 28,
        padding: '0 14px',
        borderRadius: 999,
        fontSize: 12,
        fontFamily: 'var(--ol-font-sans)',
        background: primary
          ? 'var(--ol-capsule-btn-bg-confirm)'
          : 'var(--ol-capsule-btn-bg)',
        color: 'var(--ol-capsule-btn-ink)',
        border: '0.8px solid var(--ol-capsule-btn-border)',
        boxShadow: '0 1px 2px rgba(0, 0, 0, 0.06)',
        cursor: 'default',
        transition:
          'background 0.16s var(--ol-motion-quick), transform 0.12s var(--ol-motion-quick)',
      }}
    >
      {label}
    </button>
  );
}
