import { getCurrentWindow } from '@tauri-apps/api/window';
import { createRoot } from 'react-dom/client';
import PromptBoxDemo from '../third_party/rare-ui/demo';
import './window.css';

const colorScheme = window.matchMedia('(prefers-color-scheme: dark)');
const updateTheme = () => document.documentElement.classList.toggle('dark', colorScheme.matches);
updateTheme();
colorScheme.addEventListener('change', updateTheme);

document.getElementById('close-window')?.addEventListener('click', () => {
  if ('__TAURI_INTERNALS__' in window) {
    void getCurrentWindow().close();
  } else {
    window.close();
  }
});

createRoot(document.getElementById('ask-root')!).render(<PromptBoxDemo />);
