// 「要记住这个词吗」的卡片，弹在屏幕右下角。
//
// 为什么是卡片而不是攒在词汇表页：建议是在你刚改完词的那一刻最有意义的——那时你还记得
// 自己为什么改。攒进设置页里的队列你根本想不起来去看，攒满了就开始丢最老的，等于白攒。
//
// 为什么在右下角而不是胶囊那个位置：胶囊居中在屏幕正下方，那正是你在写字的地方，卡片
// 停留十秒就把正在编辑的那一行盖住了。右下角是唯一一块「停留几秒不打扰任何人」的地方。
//
// 为什么每条都是一勾一叉、没有「全部接受」：观察器分不出「一次纠错」和「打字打到一半」
// ——真机上自动收进词汇表的 5 条里只有 1 条是对的。逐条看一眼是这里唯一可靠的判据，
// 所以不提供任何批量入口。

import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  acceptPendingCorrection,
  dismissVocabSuggestions,
  rejectPendingCorrection,
} from '../lib/ipc';
import type { PendingCorrection } from '../lib/types';

/// 卡片自己消失的时间，与后端 `VOCAB_SUGGESTION_TTL_MS` 对齐。
const TTL_MS = 10_000;

interface VocabSuggestionCardProps {
  suggestions: PendingCorrection[];
}

export function VocabSuggestionCard({ suggestions }: VocabSuggestionCardProps) {
  const { t } = useTranslation();
  // 点过的立刻从卡片上消失——不等后端回音，点了就该有反应。
  const [resolved, setResolved] = useState<Set<string>>(new Set());
  const timerRef = useRef<number | null>(null);

  // 10 秒倒计时。列表一变就重新计时：同一次听写里连着改了几个词会陆续追加进来，
  // 不重置的话后来的那条可能刚出现就没了。
  useEffect(() => {
    if (suggestions.length === 0) return;
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = window.setTimeout(() => {
      void dismissVocabSuggestions();
    }, TTL_MS);
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, [suggestions]);

  const visible = suggestions.filter(s => !resolved.has(s.id));
  if (visible.length === 0) return null;

  // 勾和叉走同一条乐观更新：先本地隐藏，失败了再放回来让用户重点一次。
  const resolve = async (id: string, commit: (id: string) => Promise<void>) => {
    setResolved(prev => new Set(prev).add(id));
    try {
      await commit(id);
    } catch {
      setResolved(prev => {
        const next = new Set(prev);
        next.delete(id);
        return next;
      });
    }
  };

  return (
    <div
      style={{
        width: '100%',
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        justifyContent: 'flex-end',
        padding: 12,
        // 卡片是唯一要接鼠标的东西——胶囊本体全程 pointerEvents:none。
        pointerEvents: 'auto',
        boxSizing: 'border-box',
        animation: 'capsule-in .28s cubic-bezier(.3,1.1,.4,1) both',
      }}
    >
      <div
        style={{
          borderRadius: 16,
          padding: 12,
          background: 'var(--ol-capsule-pill-bg)',
          backdropFilter: 'blur(20px)',
          WebkitBackdropFilter: 'blur(20px)',
          // 描边用 1px 实边 + 扩散阴影，与胶囊本体同一套写法。早期版本用
          // `0.5px solid`：非整数边框落在半个物理像素里，圆角边缘看着就是糊的。
          border: '1px solid var(--ol-capsule-pill-border)',
          boxShadow:
            'var(--ol-capsule-pill-shadow), var(--ol-capsule-pill-inset)',
          color: 'var(--ol-capsule-btn-ink)',
          fontFamily: 'var(--ol-font-sans)',
          // 子元素一律不许溢出圆角。
          overflow: 'hidden',
        }}
      >
        <div
          style={{
            fontSize: 11,
            opacity: 0.55,
            marginBottom: 10,
            letterSpacing: 0.2,
          }}
        >
          {t('vocabCard.title')}
        </div>

        <div style={{ display: 'grid', gap: 8 }}>
          {visible.map(s => (
            <div
              key={s.id}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 8,
                minWidth: 0,
                height: 28,
              }}
            >
              <span
                style={{
                  flex: 1,
                  minWidth: 0,
                  fontSize: 12.5,
                  fontFamily: 'var(--ol-font-mono)',
                  whiteSpace: 'nowrap',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                }}
                title={`${s.pattern} → ${s.replacement} · ${s.attribution} · ${s.confidence}`}
              >
                <span style={{ opacity: 0.4 }}>{s.pattern}</span>
                <span style={{ opacity: 0.3, margin: '0 5px' }}>→</span>
                {s.replacement}
              </span>
              {s.confidence === 'low' && (
                <span style={{ flexShrink: 0, fontSize: 9, opacity: 0.55 }}>
                  {t('vocabCard.lowConfidence', { defaultValue: 'check' })}
                </span>
              )}
              <CardButton
                kind="accept"
                label={t('vocabCard.accept')}
                onClick={() => void resolve(s.id, acceptPendingCorrection)}
              />
              <CardButton
                kind="reject"
                label={t('vocabCard.reject')}
                onClick={() => void resolve(s.id, rejectPendingCorrection)}
              />
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

/// 勾/叉。尺寸、配色、SVG path 都照搬胶囊上的那对确认/取消按钮 —— 同一个产品里的
/// 同一个手势，不该长成两个样子。
function CardButton({
  kind,
  label,
  onClick,
}: {
  kind: 'accept' | 'reject';
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      // 卡片浮在别的 app 上面，按下去不能把焦点从用户正在写的地方抢走。
      onMouseDown={event => {
        event.preventDefault();
        event.stopPropagation();
      }}
      aria-label={label}
      title={label}
      style={{
        width: 28,
        height: 28,
        borderRadius: 999,
        flexShrink: 0,
        padding: 0,
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        background:
          kind === 'accept'
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
      {kind === 'accept' ? (
        <svg width="13" height="13" viewBox="0 0 13 13">
          <path
            d="M2 6.5l3.2 3.5L11 3.5"
            stroke="currentColor"
            strokeWidth="1.7"
            fill="none"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      ) : (
        <svg width="11" height="11" viewBox="0 0 11 11">
          <path
            d="M1.5 1.5l8 8M9.5 1.5l-8 8"
            stroke="currentColor"
            strokeWidth="1.6"
            strokeLinecap="round"
          />
        </svg>
      )}
    </button>
  );
}
