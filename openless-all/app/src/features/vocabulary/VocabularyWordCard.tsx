import { useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { DictionaryEntry } from '../../api/contracts';
import { Icon } from '../../components/Icon';
import { Tooltip } from '../../components/Tooltip';
import { useReducedMotion } from '../../lib/motion';
import { useVocabularyLongPress } from './useVocabularyLongPress';
import { VocabularyWordHalo } from './VocabularyWordHalo';
import { VocabularyWordMenu } from './VocabularyWordMenu';

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
  onSelect: (extend: boolean) => void;
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
  const card = useRef<HTMLDivElement | null>(null);
  const button = useRef<HTMLButtonElement>(null);
  const [pointerInside, setPointerInside] = useState(false);
  const [focused, setFocused] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const lifted = menuOpen && !removing;
  const { pressing, handlers: press } = useVocabularyLongPress(
    removing || lifted,
    () => setMenuOpen(true),
    (target) => target instanceof Element && !!target.closest('.ol-word-card-action'),
  );
  const active = (pointerInside || focused) && !removing && !reduced && !lifted;

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
    <>
      <div
        {...press}
        ref={(node) => {
          card.current = node;
          cardRef(node);
        }}
        className="ol-word-card"
        data-disabled={entry.enabled ? undefined : 'true'}
        data-selected={selected ? 'true' : undefined}
        data-selecting={selecting ? 'true' : undefined}
        data-pressing={pressing && !removing ? '' : undefined}
        data-lifted={lifted ? '' : undefined}
        onPointerEnter={(event) => {
          if (event.pointerType !== 'touch') setPointerInside(true);
        }}
        onPointerLeave={() => {
          press.onPointerLeave();
          setPointerInside(false);
        }}
        onContextMenu={(event) => {
          if (removing) return;
          event.preventDefault();
          press.onPointerCancel();
          setMenuOpen(true);
        }}
        style={removing ? { pointerEvents: 'none' } : undefined}
      >
        <VocabularyWordHalo open={lifted} />
        <div className="ol-word-card-face">
          <button
            ref={button}
            type="button"
            className="ol-word-card-text"
            role={selecting ? 'checkbox' : undefined}
            aria-checked={selecting ? selected : undefined}
            aria-label={
              selecting
                ? t('vocab.selectWord', { phrase: entry.phrase })
                : entry.enabled
                  ? entry.phrase
                  : `${entry.phrase} · ${t('vocab.disabledWord')}`
            }
            aria-haspopup="menu"
            aria-expanded={lifted}
            title={reduced ? entry.phrase : undefined}
            disabled={removing}
            onClick={(event) => (selecting ? onSelect(event.shiftKey) : onToggle())}
            onFocus={(event) => setFocused(event.currentTarget.matches(':focus-visible'))}
            onBlur={() => setFocused(false)}
            onKeyDown={(event) => {
              if (event.key !== 'ContextMenu' && !(event.shiftKey && event.key === 'F10')) return;
              event.preventDefault();
              setMenuOpen(true);
            }}
          >
            <span className="ol-word-card-icon" aria-hidden="true">
              <Icon name={auto ? 'sparkle' : 'feather'} size={14} />
            </span>
            <span ref={label} className="ol-word-card-label">
              <span ref={strip} className="ol-word-card-strip">
                <span ref={text} className="ol-word-card-phrase">
                  {entry.phrase}
                </span>
                <span
                  className="ol-word-card-phrase-copy"
                  aria-hidden="true"
                  data-phrase={entry.phrase}
                />
              </span>
            </span>
            <span className="ol-word-card-hits">{entry.hits}</span>
          </button>
          {!selecting && (
            <Tooltip content={t('vocab.edit')} placement="top" className="ol-word-card-edit">
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
          )}
          {selected && (
            <span className="ol-word-card-badge" aria-hidden="true">
              <Icon name="check" size={10} strokeWidth={2.7} />
            </span>
          )}
        </div>
      </div>
      <VocabularyWordMenu
        anchor={card}
        focusTarget={button}
        label={entry.phrase}
        open={lifted}
        onOpenChange={setMenuOpen}
        onRemove={onRemove}
      />
    </>
  );
}
