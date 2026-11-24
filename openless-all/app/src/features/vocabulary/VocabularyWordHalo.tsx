import { useLayoutEffect, useRef } from 'react';
import { useExitMount } from '../../lib/useExitMount';

export function VocabularyWordHalo({ open }: { open: boolean }) {
  const { mounted, closing } = useExitMount(open, 170);
  const source = useRef<HTMLDivElement | null>(null);

  useLayoutEffect(() => {
    const layer = source.current;
    const tile = layer?.closest('.ol-word-card');
    const grid = tile?.parentElement;
    if (!open || !tile || !grid || !layer) return;

    // Mask a foreground blur instead of a backdrop filter: browser compositors
    // can ignore the gradient mask on filtered backdrops and show a hard edge.
    const region = layer.getBoundingClientRect();
    const scale = region.width / parseFloat(getComputedStyle(layer).width);
    if (!Number.isFinite(scale) || scale <= 0) return;
    const nearby = document.createDocumentFragment();
    for (const neighbor of grid.querySelectorAll<HTMLDivElement>(':scope > .ol-word-card')) {
      if (neighbor === tile) continue;
      const box = neighbor.getBoundingClientRect();
      if (box.top >= region.bottom) break;
      if (box.bottom <= region.top || box.right <= region.left || box.left >= region.right)
        continue;
      const face = neighbor.querySelector(':scope > .ol-word-card-face');
      if (!face) continue;

      const replica = document.createElement('div');
      replica.className = 'ol-word-card-replica';
      replica.dataset.disabled = neighbor.dataset.disabled ?? '';
      replica.dataset.selected = neighbor.dataset.selected ?? '';
      replica.style.left = `${(box.left - region.left) / scale}px`;
      replica.style.top = `${(box.top - region.top) / scale}px`;
      replica.style.width = `${box.width / scale}px`;
      replica.style.height = `${box.height / scale}px`;
      replica.append(face.cloneNode(true));
      replica.querySelectorAll('[id], [data-scrolling]').forEach((element) => {
        element.removeAttribute('id');
        element.removeAttribute('data-scrolling');
      });
      nearby.append(replica);
    }
    layer.replaceChildren(nearby);
  }, [open]);

  if (!mounted) return null;
  return (
    <div className="ol-word-card-halo" aria-hidden="true" data-closing={closing ? '' : undefined}>
      <div
        className="ol-word-card-halo-source"
        ref={(node) => {
          source.current = node;
          if (node) node.inert = true;
        }}
      />
    </div>
  );
}
