import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import { SiriGL } from './SiriGL';
import { getCapsulePillMetrics } from '../lib/capsuleLayout';
import type { CapsuleState } from '../lib/types';
import type { OS } from './WindowChrome';

export interface VoiceOrbStageProps {
  os: OS;
  state: CapsuleState;
  level: number;
  /** 预备态：录音光条渲染成「待命」呼吸形态，不接真实电平。见 CapsulePayload.warming。 */
  warming?: boolean;
  /** 预备→就绪的平均耗时（ms），驱动展开动画的预测节奏。见 SiriGL warmupMs。 */
  warmupMs?: number;
  message?: string;
}

/**
 * 纯光效舞台（siri-glsl 完整克隆，无壳无按钮无底）：
 *   - recording：彩虹光谱声波横贯舞台，振幅随真实麦克风电平起伏；
 *   - transcribing / polishing：波形从两端向中间收缩汇聚，流体圆点环淡入加速转动；
 *   - done / cancelled：转速回落标准，六点合并成中央一颗圆，由外层 capsule-out 淡出；
 *   - error：冻结光效 + 浮一行发光红字说明原因（唯一保留的文字信息）。
 * 刻意没有任何垫底/暗晕（用户拍板）：白底界面上宁可对比度弱，也不要黑色遮挡。
 */
export function VoiceOrbStage({
  os,
  state,
  level,
  warming,
  warmupMs,
  message,
}: VoiceOrbStageProps) {
  const { t } = useTranslation();
  const metrics = useMemo(() => getCapsulePillMetrics(os), [os]);

  // done / cancelled / error 冻结最后形态淡出，不再切换 phase。
  const lastPhaseRef = useRef<'wave' | 'orb'>('wave');
  let phase = lastPhaseRef.current;
  if (state === 'recording') phase = 'wave';
  else if (state === 'transcribing' || state === 'polishing') phase = 'orb';
  lastPhaseRef.current = phase;
  const isOrb = phase === 'orb';

  // 性能：波形淡出彻底结束（.55s delay + .6s duration）后卸载它的绘制循环 ——
  // 思考期间不再为一块不可见的 canvas 每帧跑 fragment。回到录音态立即重挂
  //（shader 编译已被驱动缓存，重建近零耗时）。
  const [waveAlive, setWaveAlive] = useState(true);
  useEffect(() => {
    if (!isOrb) {
      setWaveAlive(true);
      return undefined;
    }
    const timer = setTimeout(() => setWaveAlive(false), 1300);
    return () => clearTimeout(timer);
  }, [isOrb]);

  return (
    <div
      style={{
        width: metrics.width,
        height: metrics.height,
        boxSizing: metrics.boxSizing,
        fontFamily: 'var(--ol-font-sans)',
        position: 'relative',
        pointerEvents: 'none',
      }}
    >
      {waveAlive && (
        <SiriGL
          mode="wave"
          level={level}
          resolved={!isOrb}
          warming={warming}
          warmupMs={warmupMs}
          style={{
            position: 'absolute',
            inset: 0,
            width: '100%',
            height: '100%',
            // 收缩汇聚进行时波形保持可见，收成中央光点后再淡出，与圆点环的淡入交叠。
            opacity: isOrb ? 0 : 1,
            transition: isOrb ? 'opacity .6s ease-out .55s' : 'opacity .25s ease-out',
          }}
        />
      )}
      {isOrb && (
        <SiriGL
          mode="orb"
          // 思考中（LLM 接收）加速转动；插入/取消/出错时回落到标准速度，
          // 同时六点合并成中央一颗圆，随外层淡出一起消失。
          speed={state === 'transcribing' || state === 'polishing' ? 1.5 : 1.0}
          merging={state !== 'transcribing' && state !== 'polishing'}
          style={{
            position: 'absolute',
            left: '50%',
            top: '50%',
            width: 170,
            height: 170,
            marginLeft: -85,
            marginTop: -85,
            animation: 'siri-orb-in .7s ease-out .3s both',
          }}
        />
      )}
      {state === 'error' && <span style={errorGlowTextStyle}>{message || t('capsule.error')}</span>}
    </div>
  );
}

const errorGlowTextStyle: CSSProperties = {
  position: 'absolute',
  bottom: 24,
  left: '50%',
  transform: 'translateX(-50%)',
  maxWidth: 400,
  fontSize: 12,
  fontWeight: 600,
  lineHeight: 1.4,
  textAlign: 'center',
  color: 'var(--ol-err)',
  padding: '6px 12px',
  background: 'var(--ol-capsule-pill-bg)',
  border: '1px solid var(--ol-capsule-pill-border)',
  borderRadius: 12,
  whiteSpace: 'nowrap',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
};
