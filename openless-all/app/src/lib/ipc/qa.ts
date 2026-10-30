import type { QaHotkeyBinding, QaStatePayload } from '../types';
import { invokeOrMock } from './shared';
import { formatComboLabel } from '../hotkey';
import { mockSettings, mockSetSettings } from './mock-data';

export function getQaHotkeyLabel(): Promise<string> {
  return invokeOrMock('get_qa_hotkey_label', undefined, () =>
    mockSettings.qaCompanionKey
      ? `${formatComboLabel(mockSettings.dictationHotkey)} + ${mockSettings.qaCompanionKey}`
      : '',
  );
}

export function setQaHotkey(binding: QaHotkeyBinding | null): Promise<void> {
  return invokeOrMock('set_qa_hotkey', { binding }, () => {
    if (binding?.modifiers.length) throw new Error('只需设置一个附加键');
    const key = binding?.primary ?? '';
    if (
      [
        'Escape',
        'Fn',
        'Shift',
        'LeftShift',
        'RightShift',
        'LeftControl',
        'RightControl',
        'LeftAlt',
        'RightAlt',
        'LeftCommand',
        'RightCommand',
      ].includes(key) ||
      (key !== '' && key === mockSettings.dictationHotkey.primary)
    )
      throw new Error('请选择与录音主键不同的普通键');
    mockSetSettings({ ...mockSettings, qaHotkey: null, qaCompanionKey: key });
  });
}

export function qaWindowDismiss(): Promise<void> {
  return invokeOrMock('qa_window_dismiss', undefined, () => undefined);
}

export function qaWindowSetExpanded(expanded: boolean): Promise<void> {
  return invokeOrMock('qa_window_set_expanded', { expanded }, () => undefined);
}

export function qaToggleRecording(): Promise<void> {
  return invokeOrMock('qa_toggle_recording', undefined, () => undefined);
}

export function qaSubmitText(text: string, expectedSessionId?: string | null): Promise<void> {
  return invokeOrMock(
    'qa_submit_text',
    expectedSessionId === undefined ? { text } : { text, expectedSessionId, enforceContext: true },
    () => undefined,
  );
}

export function qaGetSnapshot(): Promise<QaStatePayload> {
  return invokeOrMock('qa_get_snapshot', undefined, () => ({ kind: 'idle', messages: [] }));
}

export function qaSetEditInstructionMode(enabled: boolean): Promise<void> {
  return invokeOrMock('qa_set_edit_instruction_mode', { enabled }, () => undefined);
}
