import { lazy, useState, type ComponentType } from 'react';
import { createRoot } from 'react-dom/client';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import { CoreStartupScreen } from '../../src/app/CoreStartupScreen';
import { TransitionViewport } from '../../src/ui/layout/TransitionViewport';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import '../../src/styles/refinements.css';

const i18n = createInstance();
await i18n.init({
  lng: 'en',
  resources: {
    en: {
      translation: {
        common: { loading: 'Loading' },
        startup: { loading: 'Starting OpenLess…', failed: 'Startup failed', recovery: 'Retry' },
      },
    },
  },
});

const SlowPage = lazy(
  () =>
    new Promise<{ default: ComponentType }>((resolve) => {
      window.setTimeout(
        () =>
          resolve({
            default: function Arrived() {
              return <h1>Arrived</h1>;
            },
          }),
        900,
      );
    }),
);

function Fixture() {
  const [page, setPage] = useState('home');
  const [booting, setBooting] = useState(true);
  return (
    <I18nextProvider i18n={i18n}>
      {booting && <CoreStartupScreen />}
      <button
        type="button"
        style={{ position: 'relative', zIndex: 90 }}
        onClick={() => {
          setBooting(false);
          setPage('next');
        }}
      >
        Switch page
      </button>
      <div style={{ display: 'flex', width: 640, height: 360 }}>
        <TransitionViewport contentKey={page}>
          {page === 'home' ? <h1>Home</h1> : <SlowPage />}
        </TransitionViewport>
      </div>
    </I18nextProvider>
  );
}

createRoot(document.getElementById('root')!).render(<Fixture />);
