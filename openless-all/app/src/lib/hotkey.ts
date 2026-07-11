import i18n from '../i18n';
import type { ComboBinding, HotkeyBinding, HotkeyTrigger, QaHotkeyBinding, ShortcutBinding } from './types';

export function defaultQaShortcut(): ShortcutBinding {
  return {
    primary: ';',
    modifiers: defaultAppShortcutModifiers(),
  };
}

export function defaultAppShortcutModifiers(): string[] {
  return currentPlatform().isMac ? ['cmd', 'shift'] : ['ctrl', 'shift'];
}

// 「停用」后重新「启用」时恢复的默认键，与后端 default_switch_style_hotkey /
// default_open_app_hotkey 保持一致（issue #576）。
export function defaultSwitchStyleShortcut(): ShortcutBinding {
  return { primary: 'S', modifiers: defaultAppShortcutModifiers() };
}

export function defaultOpenAppShortcut(): ShortcutBinding {
  return { primary: 'O', modifiers: defaultAppShortcutModifiers() };
}

export function defaultLessComputerShortcut(): ShortcutBinding {
  return { primary: 'LeftControl', modifiers: [] };
}

export function getHotkeyTriggerLabel(trigger: HotkeyTrigger | null | undefined): string {
  if (!trigger) return i18n.t('hotkey.fallback');
  if (trigger === 'custom') return i18n.t('hotkey.triggers.custom');
  return i18n.t(`hotkey.triggers.${trigger}`);
}

export function getHotkeyStartStopLabel(
  binding: HotkeyBinding | null | undefined,
  comboBinding?: ComboBinding | null,
  shortcutBinding?: ShortcutBinding | null,
): string {
  if (shortcutBinding) {
    const suffix = binding?.mode === 'hold'
      ? i18n.t('hotkey.modeHoldSuffix')
      : i18n.t('hotkey.modeToggleSuffix');
    return `${formatComboLabel(shortcutBinding)}${suffix}`;
  }
  if (binding?.trigger === 'custom' && comboBinding) {
    const combo = formatComboLabel(comboBinding);
    const suffix = binding.mode === 'hold'
      ? i18n.t('hotkey.modeHoldSuffix')
      : i18n.t('hotkey.modeToggleSuffix');
    return `${combo}${suffix}`;
  }
  const trigger = getHotkeyTriggerLabel(binding?.trigger);
  const suffix = binding?.mode === 'hold'
    ? i18n.t('hotkey.modeHoldSuffix')
    : binding?.mode === 'doubleClick'
      ? i18n.t('hotkey.modeDoubleClickSuffix')
      : i18n.t('hotkey.modeToggleSuffix');
  return `${trigger}${suffix}`;
}

export function getHotkeyUsageHint(
  binding: HotkeyBinding | null | undefined,
  comboBinding?: ComboBinding | null,
  shortcutBinding?: ShortcutBinding | null,
): string {
  if (shortcutBinding) {
    const combo = formatComboLabel(shortcutBinding);
    return binding?.mode === 'hold'
      ? i18n.t('hotkey.usageHold', { trigger: combo })
      : i18n.t('hotkey.usageToggle', { trigger: combo });
  }
  if (binding?.trigger === 'custom' && comboBinding) {
    const combo = formatComboLabel(comboBinding);
    return binding.mode === 'hold'
      ? i18n.t('hotkey.usageHold', { trigger: combo })
      : i18n.t('hotkey.usageToggle', { trigger: combo });
  }
  const trigger = getHotkeyTriggerLabel(binding?.trigger);
  return binding?.mode === 'hold'
    ? i18n.t('hotkey.usageHold', { trigger })
    : binding?.mode === 'doubleClick'
      ? i18n.t('hotkey.usageDoubleClick', { trigger })
    : i18n.t('hotkey.usageToggle', { trigger });
}

export function getHotkeyBindingCodes(binding: HotkeyBinding | null | undefined): string[] {
  if (!binding) return [];
  if (Array.isArray(binding.keys)) {
    return binding.keys.map(key => key.code.trim()).filter(Boolean);
  }
  const legacy = legacyTriggerCode(binding.trigger);
  return legacy ? [legacy] : [];
}

export function getHotkeyBindingLabel(binding: HotkeyBinding | null | undefined): string {
  const codes = getHotkeyBindingCodes(binding);
  if (codes.length === 0) return i18n.t('hotkey.unset');
  return codes.map(getHotkeyCodeLabel).join('+');
}

export function getHotkeyCodeLabel(code: string): string {
  const zh = i18n.language.toLowerCase().startsWith('zh');
  const isMac = currentPlatform().isMac;
  // Alt 在 macOS 是 Option（⌥），Meta 在 macOS 是 Command（⌘），Linux 上 Meta 是 Super。
  // 之前对所有平台一律返回 "Alt" / "Win"，QA 浮窗里 macOS 用户的"右 Option" 被显示成
  // "右 Alt"，"左 Cmd" 被显示成 "左 Win"——平台错配。下面按平台分流。
  // 用 ⌥/⌘ 与 formatPrimary 的输出（"Right ⌥" 等）保持一致。
  const labels: Record<string, string> = {
    ControlLeft: zh ? '左Ctrl' : 'Left Ctrl',
    ControlRight: zh ? '右Ctrl' : 'Right Ctrl',
    AltLeft: isMac ? (zh ? '左 ⌥' : 'Left ⌥') : (zh ? '左Alt' : 'Left Alt'),
    AltRight: isMac ? (zh ? '右 ⌥' : 'Right ⌥') : (zh ? '右Alt' : 'Right Alt'),
    ShiftLeft: zh ? '左Shift' : 'Left Shift',
    ShiftRight: zh ? '右Shift' : 'Right Shift',
    MetaLeft: isMac ? (zh ? '左 ⌘' : 'Left ⌘') : (zh ? '左Win' : 'Left Win'),
    MetaRight: isMac ? (zh ? '右 ⌘' : 'Right ⌘') : (zh ? '右Win' : 'Right Win'),
    OSLeft: isMac ? (zh ? '左 ⌘' : 'Left ⌘') : (zh ? '左Win' : 'Left Win'),
    OSRight: isMac ? (zh ? '右 ⌘' : 'Right ⌘') : (zh ? '右Win' : 'Right Win'),
    Fn: 'Fn',
    FnLock: 'FnLock',
    CapsLock: 'CapsLock',
    ScrollLock: 'ScrLock',
    Pause: 'Pause',
    PrintScreen: 'PrtSc',
    Backspace: 'Backspace',
    Tab: 'Tab',
    Enter: 'Enter',
    Space: 'Space',
    Insert: 'Insert',
    Delete: 'Delete',
    Home: 'Home',
    End: 'End',
    PageUp: 'PageUp',
    PageDown: 'PageDown',
    ArrowUp: 'Up',
    ArrowDown: 'Down',
    ArrowLeft: 'Left',
    ArrowRight: 'Right',
    ContextMenu: 'Menu',
    NumpadAdd: 'Num+',
    NumpadSubtract: 'Num-',
    NumpadMultiply: 'Num*',
    NumpadDivide: 'Num/',
    NumpadDecimal: 'Num.',
    NumpadEnter: 'NumEnter',
    Mouse4: 'Mouse4',
    Mouse5: 'Mouse5',
    Backquote: '`',
    Minus: '-',
    Equal: '=',
    BracketLeft: '[',
    BracketRight: ']',
    Backslash: '\\',
    Semicolon: ';',
    Quote: "'",
    Comma: ',',
    Period: '.',
    Slash: '/',
  };
  if (labels[code]) return labels[code];
  const letter = code.match(/^Key([A-Z])$/);
  if (letter) return letter[1];
  const digit = code.match(/^Digit([0-9])$/);
  if (digit) return digit[1];
  const numpad = code.match(/^Numpad([0-9])$/);
  if (numpad) return `Num${numpad[1]}`;
  return code;
}

function legacyTriggerCode(trigger: HotkeyTrigger | null | undefined): string | null {
  switch (trigger) {
    case 'rightOption':
    case 'rightAlt':
      return 'AltRight';
    case 'leftOption':
      return 'AltLeft';
    case 'rightControl':
      return 'ControlRight';
    case 'leftControl':
      return 'ControlLeft';
    case 'rightCommand':
      return 'MetaRight';
    case 'leftCommand':
      return 'MetaLeft';
    case 'leftShift':
      return 'ShiftLeft';
    case 'rightShift':
      return 'ShiftRight';
    case 'fn':
      return 'Fn';
    case 'mediaPlayPause':
      return 'MediaPlayPause';
    default:
      return null;
  }
}

export function shortcutFromLegacyTrigger(trigger: HotkeyTrigger): ShortcutBinding {
  const map: Record<Exclude<HotkeyTrigger, 'custom'>, string> = {
    rightOption: 'RightOption',
    leftOption: 'LeftOption',
    rightControl: 'RightControl',
    leftControl: 'LeftControl',
    rightCommand: 'RightCommand',
    leftCommand: 'LeftCommand',
    leftShift: 'LeftShift',
    rightShift: 'RightShift',
    fn: 'Fn',
    rightAlt: 'RightOption',
    mediaPlayPause: 'MediaPlayPause',
  };
  return {
    primary: map[trigger as Exclude<HotkeyTrigger, 'custom'>] ?? 'RightOption',
    modifiers: [],
  };
}

/** 把 ComboBinding 或 QaHotkeyBinding 格式化为可读标签，如 "⌘⇧D" / "Ctrl+Shift+D"。 */
/** 组合键的逐键显示段（修饰键按固定顺序 + 主键），供键帽（Kbd）逐键渲染。 */
export function formatComboParts(
  binding: ComboBinding | QaHotkeyBinding | ShortcutBinding,
): string[] {
  const parts: string[] = [];
  const platform = currentPlatform();

  // 固定输出顺序：Ctrl/Cmd → Alt/Option → Shift → Super
  const modifierOrder = [
    'cmd-left', 'cmd-right', 'cmd', 'ctrl-left', 'ctrl-right', 'ctrl',
    'alt-left', 'alt-right', 'alt', 'shift-left', 'shift-right', 'shift',
    'super-left', 'super-right', 'super',
  ] as const;
  for (const tag of modifierOrder) {
    if (binding.modifiers.some(m => m.toLowerCase() === tag)) {
      parts.push(sideModifierDisplayName(tag, platform));
    }
  }

  parts.push(formatPrimary(binding.primary));
  return parts;
}

export function formatComboLabel(binding: ComboBinding | QaHotkeyBinding | ShortcutBinding): string {
  return formatComboParts(binding).join(currentPlatform().isMac ? '' : '+');
}

export function currentPlatform(): { isMac: boolean; isWindows: boolean } {
  const nav = typeof navigator === 'undefined' ? null : navigator;
  const platform = nav?.platform || '';
  const userAgent = nav?.userAgent || '';
  return {
    isMac: platform.includes('Mac') || userAgent.includes('Mac'),
    isWindows: platform.includes('Win') || userAgent.includes('Windows'),
  };
}

/** Build side-specific modifier tags from Web KeyboardEvent.code values. */
export function sideModifiersFromPressedCodes(codes: Iterable<string>): string[] {
  const set = codes instanceof Set ? codes : new Set(codes);
  const modifiers: string[] = [];
  if (set.has('MetaLeft')) modifiers.push('cmd-left');
  else if (set.has('MetaRight')) modifiers.push('cmd-right');
  if (set.has('ControlLeft')) modifiers.push('ctrl-left');
  else if (set.has('ControlRight')) modifiers.push('ctrl-right');
  if (set.has('AltLeft')) modifiers.push('alt-left');
  else if (set.has('AltRight')) modifiers.push('alt-right');
  if (set.has('ShiftLeft')) modifiers.push('shift-left');
  else if (set.has('ShiftRight')) modifiers.push('shift-right');
  return modifiers;
}

/** Build generic modifier tags (cmd/super/ctrl/alt/shift) from pressed key codes. */
export function genericModifiersFromPressedCodes(codes: Iterable<string>): string[] {
  const set = codes instanceof Set ? codes : new Set(codes);
  const modifiers: string[] = [];
  const { isMac } = currentPlatform();
  if (isMac) {
    if (set.has('MetaLeft') || set.has('MetaRight')) modifiers.push('cmd');
  } else if (
    set.has('MetaLeft')
    || set.has('MetaRight')
    || set.has('OSLeft')
    || set.has('OSRight')
  ) {
    modifiers.push('super');
  }
  if (set.has('ControlLeft') || set.has('ControlRight')) modifiers.push('ctrl');
  if (set.has('AltLeft') || set.has('AltRight')) modifiers.push('alt');
  if (set.has('ShiftLeft') || set.has('ShiftRight')) modifiers.push('shift');
  return modifiers;
}

export function modifiersFromPressedCodes(
  codes: Iterable<string>,
  sideSpecific = false,
): string[] {
  return sideSpecific
    ? sideModifiersFromPressedCodes(codes)
    : genericModifiersFromPressedCodes(codes);
}

function modifierDisplayName(tag: string, platform: { isMac: boolean; isWindows: boolean }): string {
  return sideModifierDisplayName(tag, platform);
}

function sideModifierDisplayName(tag: string, platform: { isMac: boolean; isWindows: boolean }): string {
  const sideLabels: Record<string, string> = platform.isMac
    ? {
        'cmd-left': '左 ⌘',
        'cmd-right': '右 ⌘',
        'ctrl-left': '左 ⌃',
        'ctrl-right': '右 ⌃',
        'alt-left': '左 ⌥',
        'alt-right': '右 ⌥',
        'shift-left': '左 ⇧',
        'shift-right': '右 ⇧',
      }
    : {
        'cmd-left': platform.isWindows ? '左 Win' : '左 Super',
        'cmd-right': platform.isWindows ? '右 Win' : '右 Super',
        'ctrl-left': '左 Ctrl',
        'ctrl-right': '右 Ctrl',
        'alt-left': '左 Alt',
        'alt-right': '右 Alt',
        'shift-left': '左 Shift',
        'shift-right': '右 Shift',
        'super-left': platform.isWindows ? '左 Win' : '左 Super',
        'super-right': platform.isWindows ? '右 Win' : '右 Super',
      };
  if (sideLabels[tag]) return sideLabels[tag];
  if (platform.isMac) {
    switch (tag) {
      case 'cmd': return '\u2318';
      case 'ctrl': return '\u2303';
      case 'alt': return '\u2325';
      case 'shift': return '\u21E7';
      case 'super': return '\u2318';
    }
  } else {
    switch (tag) {
      case 'cmd': return platform.isWindows ? 'Ctrl' : 'Super';
      case 'ctrl': return 'Ctrl';
      case 'alt': return 'Alt';
      case 'shift': return 'Shift';
      case 'super': return platform.isWindows ? 'Win' : 'Super';
    }
  }
  return tag;
}

function formatPrimary(primary: string): string {
  const trimmed = primary.trim();
  if (!trimmed) return '?';
  // 单字母归大写
  if (trimmed.length === 1 && /[a-zA-Z]/.test(trimmed)) {
    return trimmed.toUpperCase();
  }
  // 常见命名键的 macOS 符号
  const isMac = currentPlatform().isMac;
  if (isMac) {
    switch (trimmed.toLowerCase()) {
      case 'space': return '\u2423';
      case 'enter':
      case 'return': return '\u21A9';
      case 'tab': return '\u21E5';
      case 'escape':
      case 'esc': return '\u238B';
      case 'backspace': return '\u232B';
      case 'delete':
      case 'del': return '\u2326';
      case 'arrowup':
      case 'up': return '\u2191';
      case 'arrowdown':
      case 'down': return '\u2193';
      case 'arrowleft':
      case 'left': return '\u2190';
      case 'arrowright':
      case 'right': return '\u2192';
    }
  }
  switch (trimmed.toLowerCase()) {
    case 'rightoption': return isMac ? 'Right ⌥' : 'Right Alt';
    case 'leftoption': return isMac ? 'Left ⌥' : 'Left Alt';
    case 'rightcontrol': return isMac ? 'Right ⌃' : 'Right Ctrl';
    case 'leftcontrol': return isMac ? 'Left ⌃' : 'Left Ctrl';
    case 'rightcommand': return isMac ? 'Right ⌘' : (currentPlatform().isWindows ? 'Right Win' : 'Right Super');
    case 'leftcommand': return isMac ? 'Left ⌘' : (currentPlatform().isWindows ? 'Left Win' : 'Left Super');
    case 'leftshift': return isMac ? 'Left ⇧' : 'Left Shift';
    case 'rightshift': return isMac ? 'Right ⇧' : 'Right Shift';
    case 'fn': return 'Fn';
    case 'mediaplaypause': return '⏯ Media';
    case 'shift': return isMac ? '⇧' : 'Shift';
  }
  return trimmed;
}
