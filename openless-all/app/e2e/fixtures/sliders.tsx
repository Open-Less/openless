import { useState, type ReactNode } from 'react';
import { createRoot } from 'react-dom/client';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import { AudioDspSettings } from '../../src/features/input/AudioDspSettings';
import { defaultAudioDsp } from '../../src/lib/audioDsp';
import {
  SimpleSlider,
  SquareSlider,
  SolidSlider,
  TinySlider,
  ReferenceSlider,
  TickSlider,
  TooltipSlider,
  RangeSlider,
  StepperSlider,
  VerticalSlider,
  DistributionSlider,
  type SliderValue,
} from '../../src/ui/controls/Slider';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';

const i18n = createInstance();
await i18n.init({
  lng: 'en',
  resources: { en: { translation: {} } },
  interpolation: { escapeValue: false },
});

function Card({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section
      aria-label={title}
      style={{
        minWidth: 0,
        border: '1px solid var(--ol-border)',
        borderRadius: 10,
        padding: '20px 24px',
        background: 'var(--ol-surface)',
      }}
    >
      <h2 style={{ fontSize: 13, margin: '0 0 20px' }}>{title}</h2>
      {children}
    </section>
  );
}
function Fixture() {
  const [value, setValue] = useState(25);
  const [changes, setChanges] = useState<SliderValue[]>([]);
  const [commits, setCommits] = useState<SliderValue[]>([]);
  const [distribution, setDistribution] = useState<readonly number[]>([20, 80]);
  const [distributionCommits, setDistributionCommits] = useState(0);
  const [dsp, setDsp] = useState({ ...defaultAudioDsp, equalizerEnabled: true });
  const [dspCommits, setDspCommits] = useState(0);
  const [dark, setDark] = useState(false);
  return (
    <div
      data-ol-theme={dark ? 'dark' : 'light'}
      style={{
        minHeight: '100vh',
        padding: 28,
        background: 'var(--ol-bg-base)',
        color: 'var(--ol-ink)',
      }}
    >
      <h1 style={{ fontSize: 22 }}>Slider library</h1>
      <button type="button" onClick={() => setDark(!dark)}>
        Toggle theme
      </button>{' '}
      <button
        type="button"
        onClick={() => {
          setValue(60);
          setDistribution([30, 70]);
        }}
      >
        External values
      </button>
      <div
        data-testid="gallery"
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(3, minmax(0, 1fr))',
          gap: 20,
          marginTop: 20,
        }}
      >
        <Card title="01 · Simple">
          <SimpleSlider aria-label="Simple" defaultValue={25} name="simple" />
        </Card>
        <Card title="03 · Square">
          <SquareSlider aria-label="Square" defaultValue={30} step={10} />
        </Card>
        <Card title="04 · Solid">
          <SolidSlider
            aria-label="Solid"
            value={value}
            onValueChange={(next) => {
              setValue(next);
              setChanges((old) => [...old, next]);
            }}
            onValueCommitted={(next) => setCommits((old) => [...old, next])}
          />
          <output data-testid="events">{JSON.stringify({ changes, commits })}</output>
        </Card>
        <Card title="05 · Tiny">
          <TinySlider aria-label="Tiny" defaultValue={25} />
        </Card>
        <Card title="06 · References">
          <ReferenceSlider
            aria-label="References"
            min={5}
            max={35}
            defaultValue={15}
            marks={[
              { value: 5, label: '5 GB' },
              { value: 20, label: '20 GB' },
              { value: 35, label: '35 GB' },
            ]}
          />
        </Card>
        <Card title="07 · Ticks">
          <TickSlider aria-label="Ticks" defaultValue={5} />
        </Card>
        <Card title="10 · Labels and tooltip">
          <TooltipSlider aria-label="Tooltip" defaultValue={50} step={10} />
        </Card>
        <Card title="11 · Dual range">
          <RangeSlider
            aria-label="Range"
            thumbLabels={['Range minimum', 'Range maximum']}
            defaultValue={[25, 75]}
          />
        </Card>
        <Card title="20 · Stepper">
          <StepperSlider aria-label="Credits" min={0} max={10} defaultValue={5} step={0.25} />
        </Card>
        <Card title="22 · Vertical">
          <VerticalSlider aria-label="Vertical" min={0} max={10} defaultValue={5} />
        </Card>
        <Card title="265 · Distribution">
          <DistributionSlider
            aria-label="Price range"
            inputLabels={['Minimum price', 'Maximum price']}
            value={distribution}
            onValueChange={setDistribution}
            onValueCommitted={() => setDistributionCommits((old) => old + 1)}
            bins={[0, 2, 4, 8, 6, 10, 7, 3, 1, 0]}
            step={0.5}
          />
          <output data-testid="distribution-commits">{distributionCommits}</output>
        </Card>
        <Card title="Runtime contracts">
          <SolidSlider
            aria-label="RTL decimal"
            direction="rtl"
            min={-1}
            max={1}
            step={0.1}
            defaultValue={0.2}
          />
          <SolidSlider aria-label="Disabled" disabled defaultValue={50} />
        </Card>
      </div>
      <Card title="Production DSP">
        <I18nextProvider i18n={i18n}>
          <AudioDspSettings
            page
            value={dsp}
            onChange={(next) => {
              setDsp(next);
              setDspCommits((old) => old + 1);
            }}
          />
        </I18nextProvider>
        <output data-testid="dsp-commits">{dspCommits}</output>
        <output data-testid="dsp-value">{JSON.stringify(dsp)}</output>
      </Card>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<Fixture />);
