import { invokeOrMock } from './shared';

/**
 * Makes the current chat-panel window the key window (gets keyboard focus).
 *
 * The QA / Less Computer windows are shown "without stealing the foreground" (macOS
 * orderFrontRegardless, never makeKey) — summoning doesn't interrupt the app the user is
 * in, at the cost that keys never reach the webview while the window isn't key (the root
 * cause of "clicked the input box but can't type"). The frontend calls this command when
 * the user clicks into the input box — at that moment taking focus is exactly what the
 * user wants.
 */
export function chatPanelFocusKeyboard(): Promise<void> {
  return invokeOrMock('chat_panel_focus_keyboard', undefined, () => undefined);
}
