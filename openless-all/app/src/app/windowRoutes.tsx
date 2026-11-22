import { lazy, Suspense } from 'react';
import { Capsule } from '../features/capsule/Capsule';
import { CapsuleTranscriptOverlay } from '../features/capsule/LiveTranscriptPill';
import { AudioCueListener } from '../features/capsule/AudioCue';
import type { OS } from '../runtime/environment';
import { QaPanel, VoiceEditPanel } from './destinations';

const MainWindow = lazy(() =>
  import('./MainWindow').then((module) => ({ default: module.MainWindow })),
);
const SelectionVoiceIntentPicker = lazy(() =>
  import('../features/selection-intent/SelectionVoiceIntentPicker').then((module) => ({
    default: module.SelectionVoiceIntentPicker,
  })),
);

export type WindowKind =
  | 'main'
  | 'capsule'
  | 'capsule-rail'
  | 'qa'
  | 'voice-edit'
  | 'selection-voice-intent'
  | 'less-computer'
  | 'less-computer-glow';

export function resolveWindowKind(params: URLSearchParams): WindowKind {
  const kind = params.get('window');
  switch (kind) {
    case 'capsule':
    case 'capsule-rail':
    case 'qa':
    case 'voice-edit':
    case 'selection-voice-intent':
    case 'less-computer':
    case 'less-computer-glow':
      return kind;
    default:
      return 'main';
  }
}

export function isCompactWindow(kind: WindowKind): boolean {
  return kind === 'capsule' || kind === 'less-computer-glow';
}

export function isDormantWindow(kind: WindowKind): boolean {
  return kind === 'less-computer' || kind === 'less-computer-glow';
}

export function WindowRoutes({
  kind,
  os,
  previewOnboarding,
}: {
  kind: WindowKind;
  os: OS;
  previewOnboarding: boolean;
}) {
  switch (kind) {
    case 'capsule-rail':
      return <CapsuleTranscriptOverlay />;
    case 'capsule':
      return (
        <>
          <Capsule os={os} />
          <AudioCueListener />
        </>
      );
    case 'qa':
      return (
        <Suspense fallback={null}>
          <QaPanel />
        </Suspense>
      );
    case 'selection-voice-intent':
      return (
        <Suspense fallback={null}>
          <SelectionVoiceIntentPicker />
        </Suspense>
      );
    case 'voice-edit':
      return (
        <Suspense fallback={null}>
          <VoiceEditPanel />
        </Suspense>
      );
    case 'less-computer':
    case 'less-computer-glow':
      return null;
    case 'main':
      return <MainWindow os={os} previewOnboarding={previewOnboarding} />;
  }
}
