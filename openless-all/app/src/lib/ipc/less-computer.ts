import { invokeOrMock } from './shared';
import type { LessComputerSyncResult, LessComputerVoiceMode } from '../types';

/** The user clicks ✕ / presses Esc to close the Less Computer overlay (hides the window). */
export function lessComputerWindowDismiss(): Promise<void> {
  return invokeOrMock('less_computer_window_dismiss', undefined, () => undefined);
}

/** Opens the text-interaction overlay from the main settings page, without first triggering the mic or a global hotkey. */
export function lessComputerWindowOpen(): Promise<void> {
  return invokeOrMock('less_computer_window_open', undefined, () => undefined);
}

/** Approve / Deny receipt of the inline approval card. The token maps to a blocked action awaiting review. */
export function lessComputerApprove(token: string, approved: boolean): Promise<void> {
  return invokeOrMock('less_computer_approve', { token, approved }, () => undefined);
}

/** Typed input in the overlay: text commands enter the Less Computer execution chain directly (same guardrails / approvals / continued sessions as voice). */
export function lessComputerSubmitText(text: string): Promise<void> {
  return invokeOrMock('less_computer_submit_text', { text }, () => undefined);
}

/** Start the mic from the panel. dictate: the transcript only fills the input box; submit: handed straight
 *  to the agent after speech (same as the hotkey). Startup failure (mic permission, another voice session
 *  in progress) rejects, shown inline in the panel. */
export function lessComputerVoiceStart(mode: LessComputerVoiceMode): Promise<void> {
  return invokeOrMock('less_computer_voice_start', { mode }, () => undefined);
}

/** Ends only the specified recording and finalizes per its mode; a late request never stops a session started later. */
export function lessComputerVoiceStop(sessionId: string): Promise<void> {
  return invokeOrMock('less_computer_voice_stop', { sessionId }, () => undefined);
}

/** Cancels only the specified recording session; other sessions and running tasks are untouched. */
export function lessComputerVoiceCancel(sessionId: string): Promise<void> {
  return invokeOrMock('less_computer_voice_cancel', { sessionId }, () => undefined);
}

/** Stops the running agent task. */
export function lessComputerTaskCancel(): Promise<void> {
  return invokeOrMock('less_computer_task_cancel', undefined, () => undefined);
}

/** On overlay mount, fetches the current session's event buffer (seq ascending) to replay events lost
 *  during webview cold load (especially the first user message — what the user said). */
export function lessComputerSync(afterSequence: number): Promise<LessComputerSyncResult> {
  return invokeOrMock('less_computer_sync', { afterSequence }, () => ({
    events: [],
    latestSequence: afterSequence,
    truncated: false,
  }));
}
