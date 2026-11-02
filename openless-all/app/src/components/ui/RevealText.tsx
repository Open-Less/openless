import type { CSSProperties } from 'react';
import { useReducedMotion } from '../../lib/motion';

/** Keep the complete text in layout and accessibility from the first frame;
 * only the painted glyphs reveal, so typing never pushes following controls. */
export function RevealText({
  text,
  active = true,
  delay = 0,
}: {
  text: string;
  active?: boolean;
  delay?: number;
}) {
  const reduced = useReducedMotion();
  if (!active || reduced) return <>{text}</>;
  let index = 0;
  const interval = Math.min(48, 2100 / Math.max(1, Array.from(text).length));
  return (
    <span
      className="ol-reveal-text"
      onAnimationEnd={(event) => {
        const glyph = event.target;
        if (glyph instanceof HTMLElement && glyph.classList.contains('ol-reveal-glyph'))
          glyph.style.animation = 'none';
      }}
    >
      <span className="ol-sr-only">{text}</span>
      <span aria-hidden="true">
        {text
          .split(/(\s+|[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}])/u)
          .filter(Boolean)
          .map((word, wordIndex) => (
            <span className="ol-reveal-word" key={wordIndex}>
              {Array.from(word).map((character, characterIndex) => (
                <span
                  className="ol-reveal-glyph"
                  key={characterIndex}
                  style={
                    { '--ol-reveal-delay': `${delay + index++ * interval}ms` } as CSSProperties
                  }
                >
                  {character}
                </span>
              ))}
            </span>
          ))}
      </span>
    </span>
  );
}
