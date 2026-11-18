import { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import { AppViewport } from '../../src/ui/layout/AppViewport';
import { InteractionProvider } from '../../src/ui/overlays/InteractionProvider';
import { SavedToast, type SaveToastState } from '../../src/ui/feedback/SavedToast';
import { emitSaved } from '../../src/lib/savedEvent';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import '../../src/styles/refinements.css';
const i18n = createInstance();
await i18n.init({ lng: 'en', resources: { en: { translation: { common: { close: 'Close' } } } } });
function Fixture() {
  const [state, setState] = useState<SaveToastState>('idle');
  const [replay, setReplay] = useState(0);
  const [acted, setActed] = useState(false);
  return (
    <>
      <SavedToast
        saveState={state}
        message="Bridge"
        replayKey={replay}
        actionLabel="Undo"
        onAction={() => setActed(true)}
      />
      <button onClick={() => setState('saving')}>Start bridge</button>
      <button onClick={() => setState('idle')}>Idle bridge</button>
      <button
        onClick={() => {
          setState('saved');
          setReplay((n) => n + 1);
        }}
      >
        Save bridge
      </button>
      <button onClick={() => emitSaved('saving', 'First saving', 'first')}>Start first</button>
      <button onClick={() => emitSaved('saving', 'Second saving', 'second')}>Start second</button>
      <button onClick={() => emitSaved('saved', 'First saved', 'first')}>Finish first</button>
      <span>{acted ? 'Action invoked' : ''}</span>
    </>
  );
}
createRoot(document.getElementById('root')!).render(
  <I18nextProvider i18n={i18n}>
    <AppViewport>
      <InteractionProvider>
        <Fixture />
      </InteractionProvider>
    </AppViewport>
  </I18nextProvider>,
);
