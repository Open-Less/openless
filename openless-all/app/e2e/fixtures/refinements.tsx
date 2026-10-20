import React, { useEffect, useState, useRef } from 'react';
import { createRoot } from 'react-dom/client';
import { AnimatePresence } from 'framer-motion';
import { i18nReady } from '../../src/i18n';
import { useSpringReorder } from '../../src/lib/useSpringReorder';
import { initInteractionMode } from '../../src/lib/interactionMode';
import { Toggle } from '../../src/pages/settings/shared';
import { Collapse, ExpandableText } from '../../src/components/ui/Disclosure';
import { SelectLite } from '../../src/components/ui/SelectLite';
import { PresenceModal } from '../../src/components/ui/Modal';
import { ChoiceChips } from '../../src/components/ui/ChoiceChips';
import { DismissibleCard } from '../../src/components/ui/DismissibleCard';
import { cardDeparture } from '../../src/lib/cardDeparture';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import '../../src/styles/refinements.css';

function Fixture() {
  const [items, setItems] = useState([
    { id: 'Alpha' },
    { id: 'Beta' },
    { id: 'Gamma' },
    { id: 'Delta' },
  ]);
  const [commits, setCommits] = useState(0);
  const reorder = useSpringReorder(items, (next) => {
    setItems(next);
    setCommits((count) => count + 1);
  });
  const [toggles, setToggles] = useState([false, true, false]);
  const [chip, setChip] = useState('one');
  const [deleted, setDeleted] = useState(false);
  const card = useRef<HTMLDivElement>(null);
  const dismiss = async () => {
    await cardDeparture(card.current ?? undefined);
    setDeleted(true);
  };
  return (
    <main style={{ overflow: 'auto', width: '100%', padding: 24 }}>
      <div
        data-testid="reorder"
        ref={reorder.list}
        style={{ display: 'flex', flexDirection: 'column', gap: 8, width: 400 }}
      >
        {items.map((item) => (
          <div
            key={item.id}
            data-row={item.id}
            ref={(el) => {
              if (el) reorder.rows.current.set(item.id, el);
              else reorder.rows.current.delete(item.id);
            }}
            style={{
              height: 64,
              background: 'var(--ol-surface-2)',
              display: 'flex',
              alignItems: 'center',
            }}
          >
            <button
              className="ol-drag-handle"
              aria-label={item.id}
              onPointerDown={(e) => reorder.onPointerDown(e, item.id)}
              onKeyDown={(e) => reorder.onKeyDown(e, item.id)}
            >
              ⠿
            </button>
            {item.id}
          </div>
        ))}
      </div>
      <output data-testid="commits">{commits}</output>
      <div data-ol-toggle-group style={{ display: 'flex', gap: 24, padding: 24 }}>
        {toggles.map((on, i) => (
          <Toggle
            key={i}
            on={on}
            label={`Switch ${i}`}
            onToggle={(next) =>
              setToggles((current) => current.map((value, index) => (i === index ? next : value)))
            }
          />
        ))}
      </div>
      <div style={{ width: 320 }}>
        <ExpandableText
          text={Array.from({ length: 20 }, (_, i) => `第 ${i + 1} 行 · Readable content.`).join(
            '\n',
          )}
        />
      </div>
      <ChoiceChips
        value={chip}
        onChange={setChip}
        label="Choices"
        options={[
          { value: 'one', label: 'One' },
          { value: 'two', label: 'Two' },
          { value: 'three', label: 'Three' },
        ]}
      />
      <AnimatePresence>
        <>
          {!deleted && (
            <DismissibleCard
              key="card"
              ref={card}
              dismissible
              onDismiss={() => void dismiss()}
              data-testid="card"
              exit={{ opacity: 0 }}
              style={{ width: 320, height: 120, padding: 18, background: 'var(--ol-surface-2)' }}
            >
              <span>Swipe this card</span>
              <button onClick={() => void dismiss()}>Delete</button>
            </DismissibleCard>
          )}
        </>
      </AnimatePresence>
      {deleted && <button onClick={() => setDeleted(false)}>Undo</button>}
      <LiveSelectFixture />
      <NoticeFixture />
      <ModalFixture />
    </main>
  );
}
function LiveSelectFixture() {
  const [pulse, setPulse] = useState(0);
  const [value, setValue] = useState('item-0');
  const [searched, setSearched] = useState('item-0');
  useEffect(() => {
    const timer = window.setInterval(() => setPulse((current) => current + 1), 120);
    return () => window.clearInterval(timer);
  }, []);
  const options = Array.from({ length: 60 }, (_, index) => ({
    value: `item-${index}`,
    label: `Option ${index}`,
    trailing: <span>{pulse}</span>,
  }));
  return (
    <section style={{ display: 'flex', gap: 20, padding: '24px 0' }}>
      <SelectLite ariaLabel="Long menu" value={value} onChange={setValue} options={options} />
      <SelectLite
        ariaLabel="Searchable menu"
        value={searched}
        onChange={setSearched}
        options={options}
        searchable
        searchPlaceholder="Search options"
      />
    </section>
  );
}
function NoticeFixture() {
  const [open, setOpen] = useState(false);
  return (
    <section style={{ width: 400 }}>
      <button onClick={() => setOpen((value) => !value)}>Toggle notice</button>
      <Collapse open={open}>
        <div data-testid="notice" style={{ height: 100, padding: 16 }}>
          Working status
        </div>
      </Collapse>
      <p data-testid="following">Following content</p>
    </section>
  );
}
function ModalFixture() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button onClick={() => setOpen(true)}>Open modal fixture</button>
      <PresenceModal
        open={open}
        labelledBy="fixture-modal-title"
        onClose={() => setOpen(false)}
        render={() => (
          <>
            <h2 id="fixture-modal-title">Modal fixture</h2>
            <button onClick={() => setOpen(false)}>Close modal fixture</button>
          </>
        )}
      />
    </>
  );
}
initInteractionMode();
void i18nReady.then(() =>
  createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <Fixture />
    </React.StrictMode>,
  ),
);
