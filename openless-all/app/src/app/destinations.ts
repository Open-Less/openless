import { lazy } from 'react';
export const loadMainShell = () => import('./shell/FloatingShell');

export const Shell = lazy(() =>
  loadMainShell().then((module) => ({ default: module.FloatingShell })),
);
export const QaPanel = lazy(() =>
  import('../pages/QaPanel').then((module) => ({ default: module.QaPanel })),
);
export const VoiceEditPanel = lazy(() =>
  import('../pages/VoiceEditPanel').then((module) => ({ default: module.VoiceEditPanel })),
);
