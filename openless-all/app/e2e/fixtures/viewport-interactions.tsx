import { StrictMode, useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { applyFontScale, type FontScaleId } from '../../src/lib/fontScale';
import { i18nReady } from '../../src/i18n';
import { GitHubActivity, type Contribution } from '../../src/ui/controls/GitHubActivity';
import { Select } from '../../src/ui/controls/Select';
import { SelectLite } from '../../src/ui/controls/SelectLite';
import { Segmented } from '../../src/ui/controls/Segmented';
import { AppViewport, PreviewFrame } from '../../src/ui/layout';
import {
  InteractionProvider,
  Modal,
  Tooltip,
  TooltipProvider,
  useInteraction,
} from '../../src/ui/overlays';

import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import '../../src/styles/overlays.css';
import '../../src/styles/refinements.css';

const params = new URLSearchParams(location.search);
const options = Array.from({ length: 48 }, (_, index) => ({
  label: `Choice ${index + 1}`,
  value: String(index),
}));
const contributions: Contribution[] = Array.from({ length: 365 }, (_, index) => ({
  date: new Date(Date.UTC(2025, 0, 1 + index)).toISOString().slice(0, 10),
  count: index,
  level: index % 4 === 0 ? 4 : 1,
}));
const apps = Array.from({ length: 35 }, (_, index) => ({
  name: `Application ${index + 1}`,
  count: 99 - index,
}));

const fixture = window as typeof window & {
  dialogResolve?: (value: string) => void;
  askAgain?: () => void;
  disposePage?: () => void;
};

function ScopedPage({ result }: { result: (message: string) => void }) {
  const { confirm } = useInteraction();
  return (
    <button
      onClick={() =>
        void confirm({ title: 'Scoped request' }).then((answer) => result(`scope:${answer}`))
      }
    >
      Request from scope
    </button>
  );
}

function Scene() {
  const [selected, setSelected] = useState('2');
  const [segment, setSegment] = useState('short');
  const [results, setResults] = useState<string[]>([]);
  const [panel, setPanel] = useState(false);
  const [scope, setScope] = useState(true);
  const interaction = useInteraction();
  const result = (entry: string) => setResults((current) => [...current, entry]);
  const ask = (title: string) =>
    void interaction
      .confirm({
        title,
        tone: 'danger',
        description:
          title === 'Long prompt'
            ? (
                <>
                  {'Long detail with translated text. '.repeat(164)}
                  <span data-testid="long-detail-end">End of the long explanation.</span>
                </>
              )
            : 'Choose carefully.',
        confirmLabel: 'Proceed with change',
        cancelLabel: 'Keep unchanged',
      })
      .then((accepted) => result(`${title}:${accepted}`));

  useEffect(() => {
    fixture.askAgain = () => ask('New prompt');
    fixture.disposePage = () => setScope(false);
    return () => {
      delete fixture.askAgain;
      delete fixture.disposePage;
    };
  });

  if (params.has('activity')) {
    return (
      <main style={{ display: 'block', width: '100%', padding: 8, overflow: 'auto' }}>
        <GitHubActivity
          contributions={contributions}
          apps={apps}
          showMonths
          heading="Yearly dictation"
          label="Most used apps by voice input"
        />
      </main>
    );
  }

  return (
    <main
      style={{
        position: 'relative',
        width: '100%',
        height: '100%',
        padding: 18,
        font: '14px sans-serif',
        color: 'var(--ol-ink)',
        background: 'var(--ol-surface)',
      }}
    >
      <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
        <button onClick={() => ask('First prompt')}>First confirmation</button>
        <button onClick={() => ask('Long prompt')}>Long confirmation</button>
        <button
          onClick={() =>
            void interaction
              .warn({ title: 'Needs attention', dismissLabel: 'Acknowledge' })
              .then((answer) => result(`warning:${answer}`))
          }
        >
          Warning
        </button>
        <button onClick={() => setPanel(true)}>Parent modal</button>
        <button
          onClick={() =>
            void interaction
              .openDialog<string>({
                title: 'Pick a result',
                render: ({ resolve }) => {
                  fixture.dialogResolve = resolve;
                  return <button onClick={() => resolve('chosen')}>Choose result</button>;
                },
              })
              .then((answer) => result(`dialog:${answer}`))
          }
        >
          Custom dialog
        </button>
        <button
          onClick={() =>
            interaction.notify({ message: 'Status only', state: 'saved', durationMs: 0 })
          }
        >
          Show feedback
        </button>
        {scope && <ScopedPage result={result} />}
      </div>
      <div aria-label="Fixture results">
        {results.map((entry, index) => (
          <div key={index}>{entry}</div>
        ))}
      </div>
      <Segmented
        value={segment}
        onChange={setSegment}
        label="Time period"
        options={[
          { value: 'short', label: 'Recent' },
          {
            value: 'long',
            label: 'Long period with an unbrokenlocalizationvalueunbrokenlocalizationvalue',
          },
        ]}
      />
      <div style={{ position: 'absolute', bottom: 22, right: 15 }}>
        <Tooltip content={'Tooltip content '.repeat(20)} placement="bottom" wrap>
          Hover for information
        </Tooltip>
        <Select
          value={selected}
          onChange={setSelected}
          options={options}
          ariaLabel="Window options"
        />
      </div>
      {panel && (
        <Modal labelledBy="parent-title" onClose={() => setPanel(false)}>
          <h2 id="parent-title">Parent dialog</h2>
          <button onClick={() => ask('Nested prompt')}>Nested confirmation</button>
          <div>
            <SelectLite
              searchable
              value={selected}
              onChange={setSelected}
              options={options}
              ariaLabel="Searchable choices"
            />
          </div>
          <button onClick={() => setPanel(false)}>Dismiss parent</button>
        </Modal>
      )}
    </main>
  );
}

applyFontScale((params.get('scale') as FontScaleId) ?? 'medium');
document.documentElement.dataset.olTheme = params.get('theme') ?? 'light';
await i18nReady;
const app = (
  <AppViewport>
    <InteractionProvider>
      <TooltipProvider>
        <Scene />
      </TooltipProvider>
    </InteractionProvider>
  </AppViewport>
);
createRoot(document.getElementById('root')!).render(
  <StrictMode>
    {params.has('frame') ? (
      <PreviewFrame width={700} height={460}>
        {app}
      </PreviewFrame>
    ) : (
      app
    )}
  </StrictMode>,
);
