import type { CredentialsStatus } from './types';
import { invokeOrMock } from './ipc/shared';
import { areProvidersConfigured, PROVIDER_SETUP_PROMPT_DEFERRED_KEY } from './providerSetup';
import { HOTKEY_MODE_MIGRATION_DEFERRED_KEY } from './hotkeyMigration';

export const DESKTOP_ONBOARDING_KEY = 'ol.desktopOnboarding.v1';
export const REOPEN_ONBOARDING_EVENT = 'openless:reopen-onboarding';
export const ONBOARDING_SESSION_KEY = 'ol.onboardingFinishedThisSession';

export interface DesktopOnboardingState {
  finished: boolean;
}

export function getDesktopOnboardingState(): Promise<DesktopOnboardingState> {
  return invokeOrMock<unknown>('get_desktop_onboarding_state', undefined, () => ({
    finished: false,
  })).then((value) => ({
    finished:
      typeof value === 'object' && value !== null && 'finished' in value
        ? (value as { finished?: unknown }).finished === true
        : false,
  }));
}

export function setDesktopOnboardingFinished(finished: boolean): Promise<void> {
  return invokeOrMock('set_desktop_onboarding_finished', { finished }, () => undefined);
}
export const GUIDE_STEPS = [
  'welcome',
  'microphone',
  'accessibility',
  'mode',
  'recognition',
  'polish',
  'shortcut',
  'finish',
] as const;
export type GuideStep = (typeof GUIDE_STEPS)[number];
export type GuideAnswer = 'done' | 'later';
export interface GuideProgress {
  version: 1;
  step: GuideStep;
  name: string;
  answers: Partial<Record<GuideStep, GuideAnswer>>;
  finished: boolean;
}
export function newGuideProgress(): GuideProgress {
  return { version: 1, step: 'welcome', name: '', answers: {}, finished: false };
}

/** This device-local record contains UI progress only, never provider credentials. */
export function parseGuideProgress(raw: string | null): GuideProgress | null {
  if (!raw) return null;
  try {
    const value = JSON.parse(raw);
    if (value?.version !== 1 || !GUIDE_STEPS.includes(value.step)) return null;
    const answers: GuideProgress['answers'] = {};
    for (const step of GUIDE_STEPS) {
      if (value.answers?.[step] === 'done' || value.answers?.[step] === 'later') {
        answers[step] = value.answers[step];
      }
    }
    return {
      version: 1,
      step: value.step,
      answers,
      name: typeof value.name === 'string' ? value.name.slice(0, 40) : '',
      finished: value.finished === true,
    };
  } catch {
    return null;
  }
}
export function readGuideProgress(): GuideProgress | null {
  try {
    return parseGuideProgress(localStorage.getItem(DESKTOP_ONBOARDING_KEY));
  } catch {
    return null;
  }
}
export function saveGuideProgress(progress: GuideProgress): boolean {
  try {
    localStorage.setItem(DESKTOP_ONBOARDING_KEY, JSON.stringify(progress));
    return true;
  } catch {
    return false;
  }
}
export function guideSteps(mac: boolean, multimodal: boolean): GuideStep[] {
  return GUIDE_STEPS.filter(
    (step) => (mac || step !== 'accessibility') && (!multimodal || step !== 'polish'),
  );
}
export function needsDesktopGuide(
  progress: GuideProgress | null,
  credentials: CredentialsStatus,
  persistedFinished = false,
): boolean {
  if (persistedFinished) return false;
  // Existing configured installations retain their normal startup. An unfinished
  // guide resumes even when credentials were configured during an earlier step.
  return progress ? !progress.finished : !areProvidersConfigured(credentials);
}
export function deferPostGuidePrompts() {
  try {
    sessionStorage.setItem(PROVIDER_SETUP_PROMPT_DEFERRED_KEY, '1');
    sessionStorage.setItem(HOTKEY_MODE_MIGRATION_DEFERRED_KEY, '1');
    sessionStorage.setItem(ONBOARDING_SESSION_KEY, '1');
  } catch {
    /* Storage denial must not prevent entering the application. */
  }
}
export function guideFinishedThisSession(): boolean {
  try {
    return sessionStorage.getItem(ONBOARDING_SESSION_KEY) === '1';
  } catch {
    return false;
  }
}
