// Heatmap.tsx — 年度活动热力图（8starlabs Heatmap 规格的纯 React 版，用户拍板）。
//
// GitHub 贡献图式布局：列 = 周（周日起），行 = 星期；顶部月份标签，左侧 Mon/Wed/Fri。
// 颜色走 interpolate 模式（minColor→maxColor 按值插值，支持 linear/sqrt/log），
// 零值格子用主题的 surface-2。提示走原生 title（date + value 的显示函数可定制）。
// 取代 PR #716 的自绘实现：数据源与历史保留策略解耦（persistence/activity.rs）。

import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';

export interface HeatmapValue {
  /** YYYY-MM-DD */
  date: string;
  value: number;
}

interface HeatmapProps {
  data: HeatmapValue[];
  startDate: Date;
  endDate: Date;
  cellSize?: number;
  gap?: number;
  /** 值→颜色的插值曲线：log 适合量级差异大的数据。 */
  interpolation?: 'linear' | 'sqrt' | 'log';
  /** 最小非零值的颜色（hex）。 */
  minColor?: string;
  /** 最大值的颜色（hex）。 */
  maxColor?: string;
  /** 星期标签：MWF = 只标 Mon/Wed/Fri。 */
  daysOfTheWeek?: 'all' | 'MWF' | 'none';
  dateDisplay?: (date: Date) => string;
  valueDisplay?: (value: number) => string;
  monthLabels?: string[];
  dayLabels?: string[];
  style?: CSSProperties;
}

const DEFAULT_MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
const DEFAULT_DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];

function isoOf(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}

function hexToRgb(hex: string): [number, number, number] {
  const value = hex.replace('#', '');
  const n = parseInt(value.length === 3 ? value.split('').map(c => c + c).join('') : value, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function mixHex(a: string, b: string, t: number): string {
  const ca = hexToRgb(a);
  const cb = hexToRgb(b);
  const mixed = ca.map((v, i) => Math.round(v + (cb[i] - v) * t));
  return `rgb(${mixed[0]}, ${mixed[1]}, ${mixed[2]})`;
}

export function Heatmap({
  data,
  startDate,
  endDate,
  cellSize = 11,
  gap = 3,
  interpolation = 'sqrt',
  minColor = '#bfdbfe',
  maxColor = '#1d4ed8',
  daysOfTheWeek = 'MWF',
  dateDisplay = (date) => date.toDateString(),
  valueDisplay = (value) => `${value}`,
  monthLabels = DEFAULT_MONTHS,
  dayLabels = DEFAULT_DAYS,
  style,
}: HeatmapProps) {
  const { weeks, max } = useMemo(() => {
    const counts = new Map(data.map(d => [d.date, d.value]));
    // 列从 startDate 所在周的周日开始，铺满到 endDate。
    const cursor = new Date(startDate);
    cursor.setDate(cursor.getDate() - cursor.getDay());
    const startIso = isoOf(startDate);
    const endIso = isoOf(endDate);
    const columns: { days: { iso: string; date: Date; value: number | null }[]; monthStart: number | null }[] = [];
    let maxValue = 0;
    let lastMonth = -1;
    // 防御上限：正常 365 天调用约 53 列，远低于此值；超大跨度时钳到 ~5 年，
    // 避免生成数千 DOM 列拖垮渲染。
    const MAX_WEEKS = 260;
    while (cursor <= endDate) {
      if (columns.length >= MAX_WEEKS) break;
      const days: { iso: string; date: Date; value: number | null }[] = [];
      let monthStart: number | null = null;
      for (let i = 0; i < 7; i += 1) {
        const date = new Date(cursor);
        const iso = isoOf(date);
        const inRange = iso >= startIso && iso <= endIso;
        const value = inRange ? counts.get(iso) ?? 0 : null;
        if (value != null && value > maxValue) maxValue = value;
        // 月份标签挂在「该月 1 日所在列」上。
        if (inRange && date.getDate() === 1 && date.getMonth() !== lastMonth) {
          monthStart = date.getMonth();
          lastMonth = date.getMonth();
        }
        days.push({ iso, date, value });
        cursor.setDate(cursor.getDate() + 1);
      }
      columns.push({ days, monthStart });
    }
    return { weeks: columns, max: maxValue };
  }, [data, startDate, endDate]);

  const colorOf = (value: number): string => {
    if (max <= 0 || value <= 0) return 'var(--ol-surface-2)';
    const ratio = value / max;
    const t =
      interpolation === 'log'
        ? Math.log1p(value) / Math.log1p(max)
        : interpolation === 'sqrt'
          ? Math.sqrt(ratio)
          : ratio;
    return mixHex(minColor, maxColor, Math.min(1, t));
  };

  const showDayLabel = (index: number): boolean => {
    if (daysOfTheWeek === 'none') return false;
    if (daysOfTheWeek === 'all') return true;
    return index === 1 || index === 3 || index === 5; // Mon / Wed / Fri
  };

  // 容器自适应（用户拍板「按比例自适应缩放」+「左右要贴边撑满」）：格子按容器宽度
  // 均分周列，让 53 列正好铺满整块卡片、右缘不留空隙。cellSize 只当窄容器下的期望值，
  // 不再作硬上限——上限会在宽容器上留出右侧空白（用户反馈的「左右没有贴边」）。
  const gridRef = useRef<HTMLDivElement>(null);
  const [fitCell, setFitCell] = useState<number | null>(null);
  const weekCount = weeks.length;
  useEffect(() => {
    const el = gridRef.current;
    if (!el || weekCount === 0) return undefined;
    const measure = () => {
      // 直接测格子区自身宽度（星期标签列在 flex 里各占各的，无需估算），
      // cell 取精确小数 —— floor 会在 53 列上累积出几十像素的右侧空隙。
      const usable = el.clientWidth;
      if (usable <= 0) return;
      const per = usable / weekCount;
      const g = per >= 13 ? 3 : 2;
      // step = cell + gap = per ⇒ weekCount × step = usable，格子精确铺满右缘。
      // 只留 5px 下限防窄容器过挤，不设上限——宽容器时格子随之放大仍贴边。
      setFitCell(Math.max(5, per - g));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [weekCount, cellSize, daysOfTheWeek]);

  const cell = fitCell ?? cellSize;
  const cellGap = cell >= 11 ? Math.min(gap, 3) : 2;
  const step = cell + cellGap;
  return (
    <div style={{ display: 'flex', gap: 6, ...style }} aria-hidden={false}>
      {daysOfTheWeek !== 'none' && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: cellGap, paddingTop: 16 }}>
          {Array.from({ length: 7 }, (_, i) => (
            <span
              key={i}
              style={{
                height: cell,
                lineHeight: `${cell}px`,
                fontSize: cell < 9 ? 8 : 9,
                color: 'var(--ol-ink-4)',
                visibility: showDayLabel(i) ? 'visible' : 'hidden',
              }}
            >
              {dayLabels[i]}
            </span>
          ))}
        </div>
      )}
      <div ref={gridRef} style={{ overflow: 'hidden', flex: 1, minWidth: 0, paddingBottom: 2 }}>
        {/* 月份标签行：绝对定位在每月起始列上方。 */}
        <div style={{ position: 'relative', height: 14, marginBottom: 2 }}>
          {weeks.map((week, i) =>
            week.monthStart != null ? (
              <span
                key={i}
                style={{
                  position: 'absolute',
                  left: i * step,
                  fontSize: cell < 9 ? 8 : 9,
                  color: 'var(--ol-ink-4)',
                  whiteSpace: 'nowrap',
                }}
              >
                {monthLabels[week.monthStart]}
              </span>
            ) : null,
          )}
        </div>
        <div style={{ display: 'flex', gap: cellGap }}>
          {weeks.map((week, wi) => (
            <div key={wi} style={{ display: 'flex', flexDirection: 'column', gap: cellGap }}>
              {week.days.map((day, di) =>
                day.value == null ? (
                  <span key={di} style={{ width: cell, height: cell }} />
                ) : (
                  <span
                    key={di}
                    title={`${dateDisplay(day.date)} · ${valueDisplay(day.value)}`}
                    style={{
                      width: cell,
                      height: cell,
                      borderRadius: cell < 8 ? 2 : 2.5,
                      background: colorOf(day.value),
                      boxShadow: 'inset 0 0 0 0.5px rgba(0,0,0,0.06)',
                    }}
                  />
                ),
              )}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
