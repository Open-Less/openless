import { StrictMode, useState } from 'react';
import { createRoot } from 'react-dom/client';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import '../../src/styles/overlays.css';
import '../../src/styles/refinements.css';

const writes: { command: string; args: unknown }[] = [];
let finishDownload: () => void = () => {};
let nextCallback = 0;
const fixture = {
  writes,
  hideManual: () => {},
  checkInBackground: () => {},
  failDownload: false,
  finishDownload: () => finishDownload(),
};
Object.assign(window, { updateFixture: fixture });
window.__TAURI_INTERNALS__ = {
  transformCallback: () => ++nextCallback,
  unregisterCallback() {},
  async invoke(command: string, args: unknown) {
    writes.push({ command, args });
    if (command === 'get_startup_snapshot')
      return { contractVersion: '2.0.0', backend: { running: true } };
    if (command === 'get_platform_capabilities')
      return { platform: 'desktop', supportsAutoUpdate: true };
    if (command === 'app_check_update_with_channel') {
      return {
        rid: 19,
        currentVersion: '2.0.1-beta.1',
        version: '2.0.1',
        rawJson: { url: 'https://example.test/update' },
      };
    }
    if (command === 'plugin:updater|download') {
      if (fixture.failDownload) throw new Error('network unavailable');
      await new Promise<void>((resolve) => {
        finishDownload = resolve;
      });
      return 42;
    }
    return null;
  },
};

const { i18nReady } = await import('../../src/i18n');
await i18nReady;
const { UpdateDialog, useAutoUpdate } = await import('../../src/components/AutoUpdate');
const { AppViewport } = await import('../../src/ui/layout/AppViewport');
const { InteractionProvider, useInteraction } =
  await import('../../src/ui/overlays/InteractionProvider');

function Updates({ background }: { background: boolean }) {
  const model = useAutoUpdate(background);
  if (background)
    fixture.checkInBackground = () => {
      void model.checkForUpdates('stable');
    };
  return (
    <>
      <button onClick={() => void model.checkForUpdates('stable')}>
        {background ? 'Background check' : 'Manual check'}
      </button>
      <UpdateDialog
        deferPrompt={background}
        {...model}
        onClose={model.dismissDialog}
        onInstall={model.installUpdate}
      />
    </>
  );
}

function Fixture() {
  const [manual, setManual] = useState(true);
  const { confirm } = useInteraction();
  fixture.hideManual = () => setManual(false);
  return (
    <div>
      <button onClick={() => void confirm({ title: 'Existing decision' })}>Other decision</button>
      <Updates background />
      {manual && <Updates background={false} />}
    </div>
  );
}

createRoot(document.querySelector('#root')!).render(
  <StrictMode>
    <AppViewport>
      <InteractionProvider>
        <Fixture />
      </InteractionProvider>
    </AppViewport>
  </StrictMode>,
);
