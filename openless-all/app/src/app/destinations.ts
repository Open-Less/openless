import { lazy } from 'react';
export const loadMainShell = () => import('./shell/FloatingShell');

export const Shell = lazy(() =>
  loadMainShell().then((module) => ({ default: module.FloatingShell })),
);
export const QaPanel = lazy(() =>
  import('../features/qa/QaPanel').then((module) => ({ default: module.QaPanel })),
);
export const VoiceEditPanel = lazy(() =>
  import('../features/voice-edit/VoiceEditPanel').then((module) => ({
    default: module.VoiceEditPanel,
  })),
);
