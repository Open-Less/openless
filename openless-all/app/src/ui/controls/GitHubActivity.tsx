'use client';

/**
 * Ported from Rare UI "GitHub Activity" (https://rareui.com/components/githubactivity).
 * Copyright (c) 2026 Swami Malode. MIT + Commons Clause; attribution required.
 * The week/month layout, tooltip and expandable panel follow upstream. Callers can fit
 * the calendar to a dashboard or expand the app list in a growing page. Local dictation
 * data and app name/count chips replace the GitHub fetch and repo avatars.
 * See docs/ATTRIBUTION.md.
 */

import * as React from 'react';
import { Tooltip as BaseTooltip } from '@base-ui/react/tooltip';
import { usePopupViewport } from '../layout/AppViewport';
import { motion, useReducedMotion, type Transition } from 'framer-motion';
import '../styles/github-activity.css';

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

const EASE_OUT = [0.22, 1, 0.36, 1] as const;
const SPRING = { type: 'spring', bounce: 0.2, duration: 0.62 } as const;
const CELL_FADE = { duration: 0.2, ease: EASE_OUT } as const;
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

type HoveredDay = { day: Contribution; anchor: HTMLElement };

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

function DayTooltip({
  hovered,
  text,
  clear,
}: {
  hovered: HoveredDay;
  text: string;
  clear: () => void;
}) {
  const { boundary, container } = usePopupViewport();
  return (
    <BaseTooltip.Root
      open
      onOpenChange={(open) => {
        if (!open) clear();
      }}
    >
      <BaseTooltip.Portal container={container}>
        <BaseTooltip.Positioner
          anchor={hovered.anchor}
          collisionBoundary={boundary}
          collisionPadding={TOOLTIP_EDGE}
          positionMethod="fixed"
          side="top"
          sideOffset={TOOLTIP_EDGE}
          className="ol-tooltip-positioner ol-gha-tooltip"
        >
          <BaseTooltip.Popup role="tooltip" className="ol-tooltip-bubble ol-gha-tooltip-bubble">
            {text}
          </BaseTooltip.Popup>
        </BaseTooltip.Positioner>
      </BaseTooltip.Portal>
    </BaseTooltip.Root>
  );
}

const ContributionGrid = ({
  contributions,
  scale,
  cellSize,
  cellGap,
  months,
  showMonths,
  label,
  monthNames,
  formatDay,
  reduceMotion,
  fitToWidth,
}: {
  contributions: Contribution[];
  scale: LevelStyle[];
  cellSize: number;
  cellGap: number;
  months: number;
  showMonths: boolean;
  label: string;
  monthNames: string[];
  formatDay: (day: Contribution) => string;
  reduceMotion: boolean | null;
  fitToWidth: boolean;
}) => {
  const weeks = React.useMemo(() => toWeeks(contributions), [contributions]);
  const gap = cellGap;
  const ref = React.useRef<HTMLDivElement>(null);
  const [hovered, setHovered] = React.useState<HoveredDay>();

  const cap = Math.min(weeks.length, weeksFor(months));
  const visible = weeks.slice(weeks.length - cap);
  const sweepEnd = (visible.length - 1) * COLUMN_STAGGER + CELL_FADE.duration;

  React.useLayoutEffect(() => {
    const scroll = ref.current;
    if (scroll) scroll.scrollLeft = fitToWidth ? 0 : scroll.scrollWidth;
    setHovered(undefined);
  }, [weeks, fitToWidth, cellSize, cellGap]);

  const hover = (day: Contribution) => (event: React.PointerEvent<HTMLElement>) => {
    setHovered({ day, anchor: event.currentTarget });
  };

  return (
    <div
      ref={ref}
      className="ol-gha-grid"
      data-fit={fitToWidth || undefined}
      tabIndex={0}
      role="img"
      aria-label={label}
      onScroll={() => setHovered(undefined)}
    >
      <div className="ol-gha-grid-inner">
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

        <div
          className="ol-gha-weeks"
          style={{ gap }}
          onPointerLeave={(event) => {
            if (event.pointerType !== 'touch') setHovered(undefined);
          }}
        >
          {visible.map((week, weekIndex) => (
            <div key={weekIndex} className="ol-gha-week" style={{ gap }}>
              {week.map((day) => (
                <motion.div
                  key={day.date}
                  onPointerEnter={(event) => {
                    if (event.pointerType !== 'touch') hover(day)(event);
                  }}
                  onPointerDown={(event) => {
                    if (event.pointerType === 'touch') hover(day)(event);
                  }}
                  className="ol-gha-cell"
                  style={{ width: cellSize, height: cellSize }}
                  initial={reduceMotion ? false : { opacity: 0, scale: 0.4 }}
                  animate={{ opacity: 1, scale: 1 }}
                  transition={{
                    ...CELL_FADE,
                    delay: reduceMotion ? 0 : weekIndex * COLUMN_STAGGER,
                  }}
                >
                  <div className="ol-gha-cell-fill" style={scale[day.level] ?? scale[0]} />
                </motion.div>
              ))}
            </div>
          ))}
        </div>
      </div>
      {hovered && (
        <DayTooltip
          hovered={hovered}
          text={formatDay(hovered.day)}
          clear={() => setHovered(undefined)}
        />
      )}
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
  /** Fit all visible weeks in the card while keeping the requested size as the upper bound. */
  fitToWidth?: boolean;
  months?: number;
  showMonths?: boolean;
  /** Panel footer label, e.g. "Top apps by dictations". */
  label?: string;
  /** Overrides the "${total} contributions in ${year}" heading. */
  heading?: React.ReactNode;
  /** Optional annual summary, laid out with the heading by the consuming dashboard. */
  summary?: React.ReactNode;
  footer?: React.ReactNode;
  /** On growing pages, expand in normal flow and leave vertical scrolling to the page. */
  inFlow?: boolean;
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
  fitToWidth = false,
  months = DEFAULT_MONTHS,
  showMonths = false,
  label = '',
  heading,
  summary,
  footer,
  inFlow = false,
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
  const cardRef = React.useRef<HTMLDivElement>(null);
  const [contentWidth, setContentWidth] = React.useState<number | null>(null);
  React.useLayoutEffect(() => {
    const card = cardRef.current;
    if (!card || !fitToWidth) return;
    // ResizeObserver reports fractional design pixels before viewport zoom rounding.
    const observer = new ResizeObserver(([entry]) => setContentWidth(entry.contentRect.width));
    observer.observe(card);
    return () => observer.disconnect();
  }, [fitToWidth]);

  const headingRef = React.useRef<HTMLDivElement>(null);
  const panelRef = React.useRef<HTMLDivElement>(null);
  const headerRef = React.useRef<HTMLDivElement>(null);
  const panelMotion = React.useRef<Animation | null>(null);
  const panelReady = React.useRef(false);

  React.useLayoutEffect(() => {
    const panel = panelRef.current;
    const header = headerRef.current;
    const card = panel?.parentElement;
    if (!panel || !header || !card) return;
    if (inFlow) {
      panelMotion.current?.cancel();
      panelMotion.current = null;
      panelReady.current = false;
      panel.style.removeProperty('height');
      return;
    }
    let target = -1;

    const measure = () => {
      const closed = header.offsetHeight;
      // Reserve the measured collapsed header even when its translated label wraps.
      card.style.setProperty('--ol-gha-panel-height', `${closed}px`);
      const inset = Number.parseFloat(getComputedStyle(panel).bottom) || 0;
      const heading = headingRef.current;
      const summaryElement = heading?.children[1];
      const reservedTop =
        heading && summaryElement && getComputedStyle(summaryElement).position !== 'absolute'
          ? heading.offsetTop + heading.offsetHeight + 8
          : inset;
      // An inline summary remains visible above an expanded app list.
      const next = open ? Math.max(closed, card.clientHeight - inset - reservedTop) : closed;
      if (next === target) return;
      target = next;
      // The animated height is in design pixels even when the viewport zooms.
      const from = Number.parseFloat(getComputedStyle(panel).height) || closed;
      panelMotion.current?.cancel();
      panelMotion.current = null;
      panel.style.height = `${target}px`;
      if (
        !panelReady.current ||
        reduceMotion ||
        Math.abs(from - target) < 0.5 ||
        typeof panel.animate !== 'function'
      ) {
        panelReady.current = true;
        return;
      }
      const animation = panel.animate([{ height: `${from}px` }, { height: `${target}px` }], {
        duration: 280,
        easing: 'cubic-bezier(0.22, 1, 0.36, 1)',
      });
      panelMotion.current = animation;
      animation.onfinish = () => {
        if (panelMotion.current === animation) panelMotion.current = null;
      };
    };

    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(card);
    observer.observe(header);
    if (headingRef.current) observer.observe(headingRef.current);
    return () => {
      observer.disconnect();
    };
  }, [open, reduceMotion, appsProp.length, inFlow]);

  React.useLayoutEffect(
    () => () => {
      panelMotion.current?.cancel();
    },
    [],
  );

  const contributions = contributionsProp;
  const apps = appsProp;
  const scale = React.useMemo(() => toScale(accent), [accent]);
  const transition = reduceMotion ? { duration: 0 } : SPRING;

  const total = React.useMemo(
    () => contributions.reduce((sum, day) => sum + day.count, 0),
    [contributions],
  );

  const parsedYear = Number(contributions[contributions.length - 1]?.date.slice(0, 4));
  const displayYear = year ?? (Number.isFinite(parsedYear) ? parsedYear : null);
  const headingText = heading ?? `${total} ${displayYear ?? ''}`.trim();

  const gap = gapFor(cellSize);
  const columns = Math.min(Math.ceil(contributions.length / 7), weeksFor(months));
  const gridWidth = columns * (cellSize + gap) - gap;
  const width = Math.max(MIN_CARD_WIDTH, gridWidth + CARD_PADDING);
  const fit =
    fitToWidth && contentWidth !== null && gridWidth > 0
      ? Math.min(1, Math.max(0, contentWidth - 1) / gridWidth)
      : 1;

  return (
    <div
      ref={cardRef}
      className={['ol-gha', apps.length > 0 && 'ol-gha--with-panel', className]
        .filter(Boolean)
        .join(' ')}
      data-in-flow={inFlow || undefined}
      style={{ width, ...style }}
    >
      <div ref={headingRef} className="ol-gha-heading-row">
        <p className="ol-gha-heading">{headingText}</p>
        {summary}
      </div>

      <ContributionGrid
        contributions={contributions}
        scale={scale}
        cellSize={cellSize * fit}
        cellGap={gap * fit}
        months={months}
        showMonths={showMonths}
        label={typeof headingText === 'string' ? headingText : label}
        monthNames={monthNames}
        formatDay={formatDay ?? ((day) => `${day.count} · ${day.date}`)}
        reduceMotion={reduceMotion}
        fitToWidth={fitToWidth}
      />

      {footer}

      {apps.length > 0 && (
        <div
          ref={panelRef}
          id={`${uid}-panel`}
          data-slot="github-activity-panel"
          data-state={open ? 'open' : 'closed'}
          className="ol-gha-panel"
        >
          <div ref={headerRef} className="ol-gha-panel-header">
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
          </div>

          <ul
            className="ol-gha-app-list"
            aria-hidden={open ? undefined : true}
            tabIndex={open && !inFlow ? 0 : undefined}
          >
            {apps.map((app) => (
              <li key={app.name}>
                <AppRow app={app} />
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
};

export { GitHubActivity };
export default GitHubActivity;
