'use client';

/**
 * Ported from Rare UI "GitHub Activity" (https://rareui.com/components/githubactivity).
 * Copyright (c) 2026 Swami Malode. MIT + Commons Clause; attribution required.
 * The upstream geometry, week/month layout, tooltip and expandable panel are kept as-is;
 * the GitHub username fetch and the repo avatars are replaced with local dictation data
 * and plain app name/count chips. See docs/ATTRIBUTION.md.
 */

import * as React from 'react';
import { createPortal } from 'react-dom';
import { AnimatePresence, motion, useReducedMotion, type Transition } from 'framer-motion';
import './github-activity.css';

export type ContributionLevel = 0 | 1 | 2 | 3 | 4;

export type Contribution = {
  date: string;
  count: number;
  level: ContributionLevel;
};

export type ActivityApp = {
  name: string;
  count: number;
};

const DEFAULT_ACCENT = '#39d353';
const DEFAULT_CELL_SIZE = 11;
const DEFAULT_MONTHS = 12;
const WEEKS_PER_MONTH = 365.25 / 12 / 7;
const STACK_LIMIT = 3;
const MIN_CARD_WIDTH = 320;
const MIN_LABEL_WEEKS = 3;
// the 16px padding on the card, both sides; the width math below has to add it back
const CARD_PADDING = 32;

const gapFor = (cellSize: number) => Math.max(2, Math.round(cellSize / 4));
// never zero: weeks.slice(-0) would hand back the whole history instead of nothing
const weeksFor = (months: number) => Math.max(1, Math.ceil(months * WEEKS_PER_MONTH));

const useIsoLayoutEffect = typeof window !== 'undefined' ? React.useLayoutEffect : React.useEffect;

const EASE_OUT = [0.22, 1, 0.36, 1] as const;
const SPRING = { type: 'spring', bounce: 0.2, duration: 0.62 } as const;
const HEADER_SPRING = { ...SPRING, bounce: 0.45 } as const;
const ROW_SPRING = { ...SPRING, bounce: 0.26, delay: 0.08 } as const;
const ROW_OFFSET = 16;
const CELL_FADE = { duration: 0.2, ease: EASE_OUT } as const;
const TOOLTIP_FADE = { duration: 0.14, ease: EASE_OUT } as const;
const TOOLTIP_EDGE = 8;
const COLUMN_STAGGER = 0.012;
const LABEL_BLUR = 6;
const LABEL_REVEAL = { duration: 0.45, ease: EASE_OUT } as const;

const LEVELS = [0, 1, 2, 3, 4] as const;

const MONTH_NAMES = [
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

function toMonthLabels(weeks: Contribution[][], monthNames: string[]) {
  const labels: (string | null)[] = weeks.map(() => null);
  const monthAt = (index: number) => weeks[index]?.[0]?.date.slice(5, 7);

  let start = 0;
  for (let i = 1; i <= weeks.length; i++) {
    if (i < weeks.length && monthAt(i) === monthAt(start)) continue;
    // a shorter run is narrower than the label itself, so it would sit under the next month
    if (i - start >= MIN_LABEL_WEEKS) {
      labels[start] = monthNames[Number(monthAt(start)) - 1] ?? null;
    }
    start = i;
  }

  return labels;
}

const LEVEL_OPACITY: Record<ContributionLevel, number> = {
  0: 0,
  1: 0.3,
  2: 0.52,
  3: 0.76,
  4: 1,
};

type LevelStyle = { backgroundColor: string; opacity: number };

type HoveredDay = { day: Contribution; x: number; y: number };

function toScale(accent: string | string[]): LevelStyle[] {
  if (typeof accent === 'string') {
    return LEVELS.map((level) => ({
      backgroundColor: accent,
      opacity: LEVEL_OPACITY[level],
    }));
  }

  const colors = accent.length > 4 ? accent : ['transparent', ...accent];
  return LEVELS.map((level) => {
    const color = colors[level] ?? colors[colors.length - 1] ?? 'transparent';
    return { backgroundColor: color, opacity: color === 'transparent' ? 0 : 1 };
  });
}

function toWeeks(contributions: Contribution[]) {
  const weeks: Contribution[][] = [];
  for (let i = 0; i < contributions.length; i += 7) {
    weeks.push(contributions.slice(i, i + 7));
  }
  return weeks;
}

function useFittedColumns(cellSize: number, gap: number) {
  const ref = React.useRef<HTMLDivElement>(null);
  const [columns, setColumns] = React.useState<number>();

  useIsoLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;

    const measure = () =>
      setColumns(Math.max(1, Math.floor((el.clientWidth + gap) / (cellSize + gap))));

    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [cellSize, gap]);

  return [ref, columns] as const;
}

const Tooltip = ({
  hovered,
  reduceMotion,
  text,
}: {
  hovered: HoveredDay;
  reduceMotion: boolean | null;
  text: string;
}) => {
  const ref = React.useRef<HTMLDivElement>(null);
  const [left, setLeft] = React.useState(hovered.x);

  useIsoLayoutEffect(() => {
    const half = (ref.current?.offsetWidth ?? 0) / 2;
    const edge = TOOLTIP_EDGE + half;
    setLeft(Math.min(Math.max(hovered.x, edge), window.innerWidth - edge));
  }, [hovered, text]);

  return createPortal(
    <div
      className="ol-gha-tooltip"
      style={{ left, top: hovered.y, transform: 'translate(-50%, calc(-100% - 8px))' }}
    >
      <motion.div
        ref={ref}
        className="ol-gha-tooltip-bubble"
        initial={reduceMotion ? false : { opacity: 0, scale: 0.94 }}
        animate={{ opacity: 1, scale: 1 }}
        exit={reduceMotion ? { opacity: 0 } : { opacity: 0, scale: 0.94 }}
        transition={reduceMotion ? { duration: 0 } : TOOLTIP_FADE}
      >
        {text}
      </motion.div>
    </div>,
    document.body,
  );
};

const ContributionGrid = ({
  contributions,
  scale,
  cellSize,
  months,
  showMonths,
  label,
  monthNames,
  formatDay,
  reduceMotion,
}: {
  contributions: Contribution[];
  scale: LevelStyle[];
  cellSize: number;
  months: number;
  showMonths: boolean;
  label: string;
  monthNames: string[];
  formatDay: (day: Contribution) => string;
  reduceMotion: boolean | null;
}) => {
  const weeks = React.useMemo(() => toWeeks(contributions), [contributions]);
  const gap = gapFor(cellSize);
  const [ref, columns] = useFittedColumns(cellSize, gap);
  const [hovered, setHovered] = React.useState<HoveredDay>();

  const cap = Math.min(weeks.length, weeksFor(months));
  const visible = weeks.slice(-Math.min(cap, columns ?? cap));
  const sweepEnd = (visible.length - 1) * COLUMN_STAGGER + CELL_FADE.duration;

  const hover = (day: Contribution) => (event: React.PointerEvent) => {
    const cell = event.currentTarget.getBoundingClientRect();
    setHovered({ day, x: cell.left + cell.width / 2, y: cell.top });
  };

  return (
    <div ref={ref} className="ol-gha-grid" role="img" aria-label={label}>
      {showMonths && (
        <motion.div
          className="ol-gha-months"
          style={{ gap, marginBottom: gap }}
          initial={reduceMotion ? false : { opacity: 0, filter: `blur(${LABEL_BLUR}px)` }}
          animate={{ opacity: 1, filter: 'blur(0px)' }}
          transition={{ ...LABEL_REVEAL, delay: reduceMotion ? 0 : sweepEnd }}
        >
          {toMonthLabels(visible, monthNames).map((month, index) => (
            <div key={index} className="ol-gha-month" style={{ width: cellSize }}>
              {month && <span className="ol-gha-month-label">{month}</span>}
            </div>
          ))}
        </motion.div>
      )}

      <div className="ol-gha-weeks" style={{ gap }} onPointerLeave={() => setHovered(undefined)}>
        {visible.map((week, weekIndex) => (
          <div key={weekIndex} className="ol-gha-week" style={{ gap }}>
            {week.map((day) => (
              <motion.div
                key={day.date}
                onPointerEnter={hover(day)}
                className="ol-gha-cell"
                style={{ width: cellSize, height: cellSize }}
                initial={reduceMotion ? false : { opacity: 0, scale: 0.4 }}
                animate={{ opacity: 1, scale: 1 }}
                transition={{ ...CELL_FADE, delay: reduceMotion ? 0 : weekIndex * COLUMN_STAGGER }}
              >
                <div className="ol-gha-cell-fill" style={scale[day.level] ?? scale[0]} />
              </motion.div>
            ))}
          </div>
        ))}
      </div>

      <AnimatePresence>
        {hovered && (
          <Tooltip
            key="tooltip"
            hovered={hovered}
            reduceMotion={reduceMotion}
            text={formatDay(hovered.day)}
          />
        )}
      </AnimatePresence>
    </div>
  );
};

const AppChip = ({ app }: { app: ActivityApp }) => (
  <span className="ol-gha-app-chip" title={app.name}>
    <span className="ol-gha-app-chip-name">{app.name}</span>
    <span className="ol-gha-app-chip-count">{app.count}</span>
  </span>
);

const AppRow = ({ app }: { app: ActivityApp }) => (
  <div className="ol-gha-app-row">
    <span className="ol-gha-app-name">{app.name}</span>
    <span className="ol-gha-app-count">{app.count}</span>
  </div>
);

const Chevron = ({ open, transition }: { open: boolean; transition: Transition }) => (
  <motion.svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.5"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden
    className="ol-gha-chevron"
    initial={false}
    animate={{ rotate: open ? 180 : 0 }}
    transition={transition}
  >
    <circle cx="12" cy="12" r="10" />
    <path d="m16 10-4 4-4-4" />
  </motion.svg>
);

export type GitHubActivityProps = {
  contributions?: Contribution[];
  apps?: ActivityApp[];
  year?: number;
  accent?: string | string[];
  cellSize?: number;
  months?: number;
  showMonths?: boolean;
  /** Panel footer label, e.g. "Top apps by dictations". */
  label?: string;
  /** Overrides the "${total} contributions in ${year}" heading. */
  heading?: React.ReactNode;
  defaultOpen?: boolean;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  /** Localized short month names, January first. */
  monthNames?: string[];
  /** Tooltip text for one day. */
  formatDay?: (day: Contribution) => string;
  hideLabel?: string;
  showLabel?: string;
  className?: string;
  style?: React.CSSProperties;
};

const GitHubActivity = ({
  className,
  contributions: contributionsProp = [],
  apps: appsProp = [],
  year,
  accent = DEFAULT_ACCENT,
  cellSize = DEFAULT_CELL_SIZE,
  months = DEFAULT_MONTHS,
  showMonths = false,
  label = '',
  heading,
  defaultOpen = false,
  open: openProp,
  onOpenChange,
  monthNames = MONTH_NAMES,
  formatDay,
  hideLabel = 'Hide top apps',
  showLabel = 'Show top apps',
  style,
}: GitHubActivityProps) => {
  const reduceMotion = useReducedMotion();
  const uid = React.useId();
  const [openState, setOpenState] = React.useState(defaultOpen);

  const open = openProp ?? openState;
  const toggle = () => {
    if (openProp === undefined) setOpenState(!open);
    onOpenChange?.(!open);
  };

  const contributions = contributionsProp;
  const apps = appsProp;
  const scale = React.useMemo(() => toScale(accent), [accent]);
  const transition = reduceMotion ? { duration: 0 } : SPRING;
  const headerTransition = reduceMotion ? { duration: 0 } : HEADER_SPRING;
  const rowTransition = reduceMotion ? { duration: 0 } : ROW_SPRING;

  const kick = reduceMotion ? {} : { x: ROW_OFFSET, y: ROW_OFFSET };
  const listMotion = {
    initial: { opacity: 0, ...kick },
    animate: { opacity: 1, x: 0, y: 0 },
    exit: { opacity: 0, ...kick },
  };

  const total = React.useMemo(
    () => contributions.reduce((sum, day) => sum + day.count, 0),
    [contributions],
  );

  const parsedYear = Number(contributions[contributions.length - 1]?.date.slice(0, 4));
  const displayYear = year ?? (Number.isFinite(parsedYear) ? parsedYear : null);
  const headingText = heading ?? `${total} ${displayYear ?? ''}`.trim();

  const gap = gapFor(cellSize);
  const columns = Math.min(Math.ceil(contributions.length / 7), weeksFor(months));
  const width = Math.max(MIN_CARD_WIDTH, columns * (cellSize + gap) - gap + CARD_PADDING);

  return (
    <div
      className={['ol-gha', apps.length > 0 && 'ol-gha--with-panel', className]
        .filter(Boolean)
        .join(' ')}
      style={{ width, ...style }}
    >
      <p className="ol-gha-heading">{headingText}</p>

      <ContributionGrid
        contributions={contributions}
        scale={scale}
        cellSize={cellSize}
        months={months}
        showMonths={showMonths}
        label={typeof headingText === 'string' ? headingText : label}
        monthNames={monthNames}
        formatDay={formatDay ?? ((day) => `${day.count} · ${day.date}`)}
        reduceMotion={reduceMotion}
      />

      {apps.length > 0 && (
        <motion.div
          layout
          id={`${uid}-panel`}
          data-slot="github-activity-panel"
          data-state={open ? 'open' : 'closed'}
          className="ol-gha-panel"
          transition={transition}
        >
          <motion.div
            layout="position"
            transition={headerTransition}
            className="ol-gha-panel-header"
          >
            <span className="ol-gha-panel-label">{label}</span>

            <div className="ol-gha-panel-actions">
              {!open && (
                <div className="ol-gha-app-chips">
                  {apps.slice(0, STACK_LIMIT).map((app) => (
                    <AppChip key={app.name} app={app} />
                  ))}
                </div>
              )}

              <button
                type="button"
                onClick={toggle}
                aria-expanded={open}
                aria-controls={`${uid}-panel`}
                aria-label={open ? hideLabel : showLabel}
                className="ol-gha-toggle"
              >
                <Chevron open={open} transition={transition} />
              </button>
            </div>
          </motion.div>

          <AnimatePresence initial={false} mode="popLayout">
            {open && (
              <motion.ul
                key="list"
                layout="position"
                {...listMotion}
                transition={rowTransition}
                className="ol-gha-app-list"
              >
                {apps.map((app) => (
                  <li key={app.name}>
                    <AppRow app={app} />
                  </li>
                ))}
              </motion.ul>
            )}
          </AnimatePresence>
        </motion.div>
      )}
    </div>
  );
};

export { GitHubActivity };
export default GitHubActivity;
