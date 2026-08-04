// 「要记住这个词吗」的卡片，弹在胶囊那个位置。
//
// 为什么是卡片而不是攒在词汇表页：建议是在你刚改完词的那一刻最有意义的——那时你还记得
// 自己为什么改。攒进设置页里的队列你根本想不起来去看，攒满了就开始丢最老的，等于白攒。
//
// 为什么弹在胶囊那儿：那个窗口是不抢焦点的浮窗，你在别的 app 里打字时它出现不会把光标
// 夺走；而且它的位置你已经习惯往那儿看。

import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { acceptPendingCorrection, dismissVocabSuggestions } from '../lib/ipc';
import type { PendingCorrection } from '../lib/types';

/// 卡片自己消失的时间，与后端 `VOCAB_SUGGESTION_TTL_MS` 对齐。
const TTL_MS = 10_000;

interface VocabSuggestionCardProps {
  suggestions: PendingCorrection[];
}

export function VocabSuggestionCard({ suggestions }: VocabSuggestionCardProps) {
  const { t } = useTranslation();
  // 已经点过「好」的，立刻从卡片上消失——不等后端回音，点了就该有反应。
  const [accepted, setAccepted] = useState<Set<string>>(new Set());
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

  const visible = suggestions.filter(s => !accepted.has(s.id));
  if (visible.length === 0) return null;

  const onAccept = async (id: string) => {
    setAccepted(prev => new Set(prev).add(id));
    try {
      await acceptPendingCorrection(id);
    } catch {
      // 失败就把它放回来，让用户能再点一次。
      setAccepted(prev => {
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
        // 卡片是唯一要接鼠标的东西——胶囊本体全程 pointerEvents:none。
        pointerEvents: 'auto',
        animation: 'capsule-in .28s cubic-bezier(.3,1.1,.4,1) both',
      }}
    >
      <div
        style={{
          borderRadius: 14,
          padding: '10px 12px 12px',
          background: 'rgba(28, 30, 38, 0.92)',
          backdropFilter: 'blur(20px)',
          WebkitBackdropFilter: 'blur(20px)',
          border: '0.5px solid rgba(255, 255, 255, 0.14)',
          boxShadow: '0 8px 32px rgba(0, 0, 0, 0.45)',
          fontFamily: 'var(--ol-font-sans)',
        }}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            marginBottom: 8,
          }}
        >
          <span style={{ fontSize: 11, color: 'rgba(255,255,255,0.55)' }}>
            {t('vocabCard.title')}
          </span>
          <button
            onClick={() => void dismissVocabSuggestions()}
            style={{
              background: 'transparent',
              border: 0,
              padding: '2px 4px',
              fontSize: 11,
              color: 'rgba(255,255,255,0.45)',
              cursor: 'default',
              fontFamily: 'inherit',
            }}
          >
            {t('vocabCard.dismissAll')}
          </button>
        </div>

        <div style={{ display: 'grid', gap: 6 }}>
          {visible.map(s => (
            <div
              key={s.id}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 8,
                minWidth: 0,
              }}
            >
              <span
                style={{
                  flex: 1,
                  minWidth: 0,
                  fontSize: 12.5,
                  fontFamily: 'var(--ol-font-mono)',
                  color: 'rgba(255,255,255,0.92)',
                  whiteSpace: 'nowrap',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                }}
                title={`${s.pattern} → ${s.replacement}`}
              >
                <span style={{ color: 'rgba(255,255,255,0.4)' }}>{s.pattern}</span>
                <span style={{ color: 'rgba(255,255,255,0.3)', margin: '0 5px' }}>→</span>
                {s.replacement}
              </span>
              <button
                onClick={() => void onAccept(s.id)}
                style={{
                  flexShrink: 0,
                  padding: '3px 12px',
                  borderRadius: 999,
                  border: 0,
                  background: 'rgba(90, 140, 255, 0.9)',
                  color: '#fff',
                  fontSize: 11.5,
                  fontWeight: 600,
                  cursor: 'default',
                  fontFamily: 'inherit',
                }}
              >
                {t('vocabCard.accept')}
              </button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
