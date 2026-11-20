import { createRoot } from 'react-dom/client';
import { AskWindow } from './ask-window';
import { tauriAskClient } from './tauri-client';

createRoot(document.getElementById('ask-root')!).render(<AskWindow client={tauriAskClient} />);
