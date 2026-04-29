import type { HotkeyBinding, HotkeyTrigger } from './types';

export const HOTKEY_TRIGGER_LABEL: Record<HotkeyTrigger, string> = {
  rightOption: '右 Option',
  leftOption: '左 Option',
  rightControl: '右 Control',
  leftControl: '左 Control',
  rightCommand: '右 Command',
  fn: 'Fn (地球键)',
  rightAlt: '右 Alt',
};

export function getHotkeyTriggerLabel(trigger: HotkeyTrigger | null | undefined): string {
  return trigger ? HOTKEY_TRIGGER_LABEL[trigger] : '全局快捷键';
}

export function getHotkeyStartStopLabel(binding: HotkeyBinding | null | undefined): string {
  const trigger = getHotkeyTriggerLabel(binding?.trigger);
  return binding?.mode === 'hold' ? `${trigger}（按住说话）` : `${trigger}（开始 / 停止）`;
}

export function getHotkeyUsageHint(binding: HotkeyBinding | null | undefined): string {
  const trigger = getHotkeyTriggerLabel(binding?.trigger);
  return binding?.mode === 'hold' ? `按住 ${trigger} 说话，松开结束。` : `按 ${trigger} 开始录音，再按一次结束。`;
}
