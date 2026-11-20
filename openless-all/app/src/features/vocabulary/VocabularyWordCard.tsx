import { useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { DictionaryEntry } from '../../api/contracts';
import { Icon } from '../../components/Icon';
import { Tooltip } from '../../components/Tooltip';
import { useReducedMotion } from '../../lib/motion';

interface WordCardProps {
  entry: DictionaryEntry;
  auto: boolean;
  removing: boolean;
  cardRef: (element: HTMLDivElement | null) => void;
  onToggle: () => void;
  onEdit: () => void;
  onRemove: () => void;
  selected: boolean;
  selecting: boolean;
  onSelect: () => void;
}

export function VocabularyWordCard({
  entry,
  auto,
  removing,
  selected,
  selecting,
  cardRef,
  onToggle,
  onEdit,
  onRemove,
  onSelect,
}: WordCardProps) {
  const { t } = useTranslation();
  const reduced = useReducedMotion();
  const label = useRef<HTMLSpanElement>(null);
  const strip = useRef<HTMLSpanElement>(null);
  const text = useRef<HTMLSpanElement>(null);
  const [pointerInside, setPointerInside] = useState(false);
  const [focused, setFocused] = useState(false);
  const active = (pointerInside || focused) && !removing && !reduced;

  useLayoutEffect(() => {
    const viewport = label.current;
    const track = strip.current;
    const value = text.current;
    if (!active || !viewport || !track || !value || typeof track.animate !== 'function') return;

    let animation: Animation | undefined;
    let lastWidth = -1;
    const reset = () => {
      animation?.cancel();
      animation = undefined;
      delete viewport.dataset.scrolling;
    };
    const measure = () => {
      // Measure in local CSS pixels so interface zoom never shortens the reveal.
      const width = value.offsetWidth;
      if (width <= viewport.clientWidth + 1 || viewport.clientWidth === 0) {
        lastWidth = -1;
        reset();
        return;
      }
      if (width === lastWidth && animation) return;
      lastWidth = width;
      animation?.cancel();
      viewport.dataset.scrolling = '';
      const gap = parseFloat(getComputedStyle(track).columnGap) || 0;
      const sign = getComputedStyle(viewport).direction === 'rtl' ? 1 : -1;
      // Half the track plus half the gap lines up the second copy exactly with the first.
      const end = `translateX(calc(${sign * 50}% + ${(sign * gap) / 2}px))`;
      animation = track.animate([{ transform: 'translateX(0px)' }, { transform: end }], {
        duration: ((width + gap) / 55) * 1000,
        iterations: Infinity,
        easing: 'linear',
      });
    };
    measure();
    const observer = typeof ResizeObserver === 'function' ? new ResizeObserver(measure) : undefined;
    observer?.observe(viewport);
    observer?.observe(value);
    return () => {
      observer?.disconnect();
      reset();
    };
  }, [active, entry.phrase]);

  return (
    <div
      ref={cardRef}
      className="ol-word-card"
      data-disabled={entry.enabled ? undefined : 'true'}
      data-selected={selected ? 'true' : undefined}
      data-selecting={selecting ? 'true' : undefined}
      onPointerEnter={(event) => {
        if (event.pointerType !== 'touch') setPointerInside(true);
      }}
      onPointerLeave={() => setPointerInside(false)}
      style={removing ? { pointerEvents: 'none' } : undefined}
    >
      <span
        className="ol-word-card-icon"
        role={entry.enabled ? undefined : 'img'}
        aria-hidden={entry.enabled || undefined}
        aria-label={entry.enabled ? undefined : t('vocab.disabledWord')}
        title={entry.enabled ? undefined : t('vocab.disabledWord')}
      >
        <Icon name={auto ? 'sparkle' : 'feather'} size={14} />
      </span>
      <button
        type="button"
        className="ol-word-card-text"
        aria-label={entry.enabled ? undefined : `${entry.phrase} · ${t('vocab.disabledWord')}`}
        disabled={removing}
        onClick={onToggle}
        onFocus={() => setFocused(true)}
        onBlur={() => setFocused(false)}
        title={entry.phrase}
      >
        <span ref={label} className="ol-word-card-label">
          <span ref={strip} className="ol-word-card-strip">
            <span ref={text} className="ol-word-card-phrase">
              {entry.phrase}
            </span>
            <span className="ol-word-card-phrase-copy" aria-hidden="true">
              {entry.phrase}
            </span>
          </span>
        </span>
      </button>
      <span className="ol-word-card-hits">{entry.hits}</span>
      <span className="ol-word-card-actions">
        <Tooltip content={t('vocab.edit')} placement="top">
          <button
            type="button"
            className="ol-word-card-action"
            aria-label={t('vocab.edit')}
            disabled={removing}
            onClick={onEdit}
          >
            <Icon name="pencil" size={14} />
          </button>
        </Tooltip>
        <Tooltip content={t('common.delete')} placement="top">
          <button
            type="button"
            className="ol-word-card-action"
            aria-label={t('vocab.deleteWord', { phrase: entry.phrase })}
            disabled={removing}
            onClick={onRemove}
          >
            <Icon name="trash" size={14} />
          </button>
        </Tooltip>
      </span>
      <input
        type="checkbox"
        className="ol-word-card-select"
        checked={selected}
        disabled={removing}
        aria-label={t('vocab.selectWord', { phrase: entry.phrase })}
        onChange={onSelect}
      />
    </div>
  );
}
