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
  const text = useRef<HTMLSpanElement>(null);
  const [pointerInside, setPointerInside] = useState(false);
  const [focused, setFocused] = useState(false);
  const active = (pointerInside || focused) && !removing && !reduced;

  useLayoutEffect(() => {
    const viewport = label.current;
    const value = text.current;
    if (!active || !viewport || !value || typeof value.animate !== 'function') return;

    let animation: Animation | undefined;
    let lastDistance = -1;
    const reset = () => {
      animation?.cancel();
      animation = undefined;
      delete viewport.dataset.scrolling;
    };
    const measure = () => {
      // Measure in local CSS pixels so interface zoom never shortens the reveal.
      const distance = value.offsetWidth - viewport.clientWidth;
      if (distance === lastDistance) return;
      lastDistance = distance;
      reset();
      if (distance <= 1 || viewport.clientWidth === 0) return;
      const end = `translateX(${getComputedStyle(viewport).direction === 'rtl' ? distance : -distance}px)`;
      const travel = Math.max(500, (distance / 45) * 1000);
      const duration = travel * 2 + 1250;
      const start = 'translateX(0px)';
      viewport.dataset.scrolling = '';
      animation = value.animate(
        [
          { transform: start, offset: 0 },
          { transform: start, offset: 450 / duration },
          { transform: end, offset: (450 + travel) / duration },
          { transform: end, offset: (1250 + travel) / duration },
          { transform: start, offset: 1 },
        ],
        { duration, iterations: Infinity },
      );
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
        onPointerEnter={() => setPointerInside(true)}
        onPointerLeave={() => setPointerInside(false)}
        onFocus={() => setFocused(true)}
        onBlur={() => setFocused(false)}
        title={entry.phrase}
      >
        <span ref={label} className="ol-word-card-label">
          <span ref={text} className="ol-word-card-phrase">
            {entry.phrase}
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
