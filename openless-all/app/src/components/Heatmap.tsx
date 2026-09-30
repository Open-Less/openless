// Heatmap.tsx — yearly activity heatmap (pure React take on the 8starlabs Heatmap spec).
//
// GitHub-contribution-graph layout: columns = weeks (starting Sunday), rows = weekdays;
// month labels on top, Mon/Wed/Fri on the left. Colors interpolate minColor→maxColor
// by value (linear/sqrt/log); zero cells use the theme's surface-2. Tooltips are native
// title with customizable date/value display. Replaces the PR #716 hand-drawn version:
// the data source is decoupled from history retention (persistence/activity.rs).

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
  /** Cell size cap: the overview fixed page must bound total heatmap height; no unlimited growth in wide containers. */
  maxCellSize?: number;
  /** Value→color interpolation curve: log suits data with large magnitude ranges. */
  interpolation?: 'linear' | 'sqrt' | 'log';
  /** Color (hex) for the smallest non-zero value. */
  minColor?: string;
  /** Color (hex) for the max value. */
  maxColor?: string;
  /** Weekday labels: MWF = only Mon/Wed/Fri. */
  daysOfTheWeek?: 'all' | 'MWF' | 'none';
  dateDisplay?: (date: Date) => string;
  valueDisplay?: (value: number) => string;
  monthLabels?: string[];
  dayLabels?: string[];
  style?: CSSProperties;
}

const DEFAULT_MONTHS = [
  'Jan',
  'Feb',
  'Mar',
  'Apr',
  'May',
  'Jun',
  'Jul',
  'Aug',
  'Sep',
  'Oct',
  'Nov',
  'Dec',
];
const DEFAULT_DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];

function isoOf(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}

function hexToRgb(hex: string): [number, number, number] {
  const value = hex.replace('#', '');
  const n = parseInt(
    value.length === 3
      ? value
          .split('')
          .map((c) => c + c)
          .join('')
      : value,
    16,
  );
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
  maxCellSize,
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
    const counts = new Map(data.map((d) => [d.date, d.value]));
    // Columns start on the Sunday of startDate's week and fill through endDate.
    const cursor = new Date(startDate);
    cursor.setDate(cursor.getDate() - cursor.getDay());
    const startIso = isoOf(startDate);
    const endIso = isoOf(endDate);
    const columns: {
      days: { iso: string; date: Date; value: number | null }[];
      monthStart: number | null;
    }[] = [];
    let maxValue = 0;
    let lastMonth = -1;
    // Defensive cap: a normal 365-day range is ~53 columns, well below this; clamp
    // huge spans to ~5 years so thousands of DOM columns can't grind rendering down.
    const MAX_WEEKS = 260;
    while (cursor <= endDate) {
      if (columns.length >= MAX_WEEKS) break;
      const days: { iso: string; date: Date; value: number | null }[] = [];
      let monthStart: number | null = null;
      for (let i = 0; i < 7; i += 1) {
        const date = new Date(cursor);
        const iso = isoOf(date);
        const inRange = iso >= startIso && iso <= endIso;
        const value = inRange ? (counts.get(iso) ?? 0) : null;
        if (value != null && value > maxValue) maxValue = value;
        // Month labels attach to the column containing the 1st of the month.
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

  // Container-adaptive sizing (proportional scaling, must fill edge to edge): cells
  // divide the container width evenly across week columns so 53 columns exactly fill
  // the card with no gap at the right edge. cellSize is only the preferred size in
  // narrow containers, no longer a hard cap — a cap leaves right-side blank space in
  // wide containers (the "not filling edge to edge" feedback).
  const gridRef = useRef<HTMLDivElement>(null);
  const [fitCell, setFitCell] = useState<number | null>(null);
  const weekCount = weeks.length;
  useEffect(() => {
    const el = gridRef.current;
    if (!el || weekCount === 0) return undefined;
    const measure = () => {
      // Measure the grid area's own width (day-label columns are separate flex items,
      // no estimation needed); keep cell as an exact fraction — floor accumulates tens
      // of pixels of right-side gap across 53 columns.
      const usable = el.clientWidth;
      if (usable <= 0) return;
      const per = usable / weekCount;
      const g = per >= 13 ? 3 : 2;
      // step = cell + gap = per ⇒ weekCount × step = usable, so cells exactly fill to
      // the right edge. 5px floor prevents crushing in narrow containers; maxCellSize
      // caps the overview fixed page (unset: cells grow with wide containers and stay
      // flush).
      const fitted = Math.max(5, per - g);
      setFitCell(maxCellSize != null ? Math.min(fitted, maxCellSize) : fitted);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [weekCount, cellSize, daysOfTheWeek, maxCellSize]);

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
        {/* Month label row: absolutely positioned above each month's starting column. */}
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
