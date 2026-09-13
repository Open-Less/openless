import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import { SplashVideo } from './components/SplashVideo';
import { detectOS } from './components/WindowChrome';
import { i18nReady } from './i18n';
import { initThemeMode } from './lib/themeMode';
import './styles/tokens.css';
import './styles/global.css';

import type { OS } from './components/WindowChrome';

const params = new URLSearchParams(window.location.search);
const windowKind = params.get('window');
const isCapsule = windowKind === 'capsule';
const isQa = windowKind === 'qa';
const isSelectionPolishPreview = windowKind === 'selection-polish-preview';
const isSelectionVoiceIntent = windowKind === 'selection-voice-intent';
const isLessComputer = windowKind === 'less-computer';
const isLessComputerGlow = windowKind === 'less-computer-glow';
// 开屏 PV 只属于主窗口（无 ?window= 参数的路由）：胶囊 / QA / Less Computer 等
// 辅助窗口共用同一份前端产物，但绝不能抢占或重复消费开屏。
const isMainWindow = !windowKind;
const osQuery = params.get('os') as OS | null;
const os = osQuery ?? detectOS();
document.documentElement.dataset.olPlatform = os;
initThemeMode();

const root = ReactDOM.createRoot(document.getElementById('root')!);

const renderApp = () => {
  root.render(
    <React.StrictMode>
      {isMainWindow && <SplashVideo />}
      <App
        isCapsule={isCapsule}
        isQa={isQa}
        isSelectionPolishPreview={isSelectionPolishPreview}
        isSelectionVoiceIntent={isSelectionVoiceIntent}
        isLessComputer={isLessComputer}
        isLessComputerGlow={isLessComputerGlow}
        forcedOs={os}
      />
    </React.StrictMode>,
  );
};

// Mount only after the selected local language chunk is ready; avoid mixed-language startup.
void i18nReady.then(renderApp);
