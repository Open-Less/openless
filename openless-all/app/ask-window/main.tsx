import { createRoot } from 'react-dom/client';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { AskWindow } from './ask-window';
import { tauriAskClient } from './tauri-client';

const surface =
  getCurrentWebviewWindow().label === 'ask-conversation' ? 'conversation' : 'composer';
createRoot(document.getElementById('ask-root')!).render(
  <AskWindow client={tauriAskClient} surface={surface} />,
);
