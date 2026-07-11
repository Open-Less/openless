import { useEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { formatComboParts, getHotkeyTriggerLabel, modifiersFromPressedCodes, shortcutFromLegacyTrigger } from '../lib/hotkey';
import { KbdGroup } from './Kbd';
import { setShortcutRecordingActive, validateShortcutBinding } from '../lib/ipc';
import type { HotkeyTrigger, ShortcutBinding } from '../lib/types';

export function ShortcutRecorder({
  value,
  onSave,
  alignRecordButton = false,
  disabled = false,
  onDisable,
  disableLabel,
  comboOnly = false,
  modifierPresets = [],
  sideSpecificModifiers = false,
}: {
  value: ShortcutBinding;
  onSave: (binding: ShortcutBinding) => Promise<void>;
  alignRecordButton?: boolean;
  disabled?: boolean;
  /** 提供则在「录制」按钮左侧并排渲染一个「停用」旋钮。 */
  onDisable?: () => void | Promise<void>;
  disableLabel?: string;
  /** 仅允许组合键（修饰键+主键 / 功能键）；拒绝单修饰键，因为全局热键无法注册它。 */
  comboOnly?: boolean;
  /** 平台可用的单修饰键快捷选项（如 Fn，WebView 无法录制）。 */
  modifierPresets?: HotkeyTrigger[];
  /** 听写 start/stop 专用：录制 cmd-left / ctrl-right 等侧向修饰键。 */
  sideSpecificModifiers?: boolean;
}) {
  const { t } = useTranslation();
  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pendingModifier = useRef<ShortcutBinding | null>(null);
  const pendingTimer = useRef<number | null>(null);
  const pressedCodes = useRef<Set<string>>(new Set());

  const clearPressedCodes = () => {
    pressedCodes.current.clear();
  };

  const clearPendingModifier = () => {
    if (pendingTimer.current !== null) {
      window.clearTimeout(pendingTimer.current);
      pendingTimer.current = null;
    }
    pendingModifier.current = null;
  };

  const resetRecordingState = () => {
    clearPendingModifier();
    clearPressedCodes();
  };

  useEffect(() => () => {
    resetRecordingState();
    void setShortcutRecordingActive(false);
  }, []);

  useEffect(() => {
    void setShortcutRecordingActive(recording);
    return () => {
      if (recording) void setShortcutRecordingActive(false);
    };
  }, [recording]);

  useEffect(() => {
    if (!disabled || !recording) return;
    setRecording(false);
    resetRecordingState();
  }, [disabled, recording]);

  const finish = async (binding: ShortcutBinding) => {
    try {
      await validateShortcutBinding(binding);
      await onSave(binding);
      resetRecordingState();
      setRecording(false);
      setError(null);
    } catch {
      setError(t('settings.recording.comboConflict'));
    }
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (!recording || disabled) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') {
      setRecording(false);
      setError(null);
      resetRecordingState();
      return;
    }
    if (isModifierKey(e.key)) {
      pressedCodes.current.add(e.code);
      if (comboOnly) {
        return;
      }
      const primary = modifierPrimaryFromCode(e.code, e.key);
      if (!primary || pendingModifier.current?.primary === primary) return;
      clearPendingModifier();
      const binding = { primary, modifiers: [] };
      pendingModifier.current = binding;
      pendingTimer.current = window.setTimeout(() => {
        if (pendingModifier.current?.primary === primary) {
          void finish(binding);
        }
      }, 650);
      return;
    }
    clearPendingModifier();
    const primary = primaryFromKeyboardEvent(e);
    if (primary) {
      void finish({ primary, modifiers: modifiersFromPressedCodes(pressedCodes.current, sideSpecificModifiers) });
    }
  };

  const onKeyUp = (e: KeyboardEvent<HTMLDivElement>) => {
    if (!recording || disabled || !isModifierKey(e.key)) return;
    e.preventDefault();
    e.stopPropagation();
    pressedCodes.current.delete(e.code);
    if (comboOnly) return;
    const primary = modifierPrimaryFromCode(e.code, e.key);
    if (primary && pendingModifier.current?.primary === primary) {
      const binding = pendingModifier.current;
      clearPendingModifier();
      void finish(binding);
    }
  };

  const rootStyle: CSSProperties = {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
    width: alignRecordButton ? '100%' : undefined,
  };
  const recorderRowStyle: CSSProperties = {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    flexWrap: 'wrap',
    width: alignRecordButton ? '100%' : undefined,
  };
  const recordButtonStyle: CSSProperties = {
    fontSize: 12,
    padding: '5px 14px',
    background: recording ? 'rgba(37,99,235,0.12)' : 'var(--ol-blue)',
    color: recording ? 'var(--ol-blue)' : '#fff',
    border: 0,
    borderRadius: 6,
    fontFamily: 'inherit',
    fontWeight: 500,
    cursor: recording || disabled ? 'default' : 'pointer',
    opacity: disabled ? 0.68 : 1,
  };
  const disableKnobStyle: CSSProperties = {
    fontSize: 12,
    padding: '5px 12px',
    background: 'transparent',
    color: 'var(--ol-ink-4)',
    border: '0.5px solid var(--ol-line-strong)',
    borderRadius: 6,
    fontFamily: 'inherit',
    fontWeight: 500,
    cursor: recording ? 'default' : 'pointer',
  };
  const controlsGroupStyle: CSSProperties = {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 8,
    marginLeft: alignRecordButton ? 'auto' : undefined,
  };
  const presetChipStyle: CSSProperties = {
    fontSize: 11,
    padding: '4px 10px',
    borderRadius: 999,
    border: '0.5px solid var(--ol-line-strong)',
    background: 'transparent',
    color: 'var(--ol-ink-3)',
    fontFamily: 'inherit',
    cursor: disabled || recording ? 'default' : 'pointer',
  };

  const presetTriggers = modifierPresets.filter(t => t !== 'custom' && t !== 'mediaPlayPause');

  return (
    <div style={rootStyle}>
      <div style={recorderRowStyle}>
        {/* 键帽逐键展示（Kbd 组件，用户拍板的快捷键展示标准），替代整块灰底文本。 */}
        <KbdGroup keys={formatComboParts(value)} />
        <div style={controlsGroupStyle}>
          {onDisable && (
            <button
              onClick={() => {
                if (recording) return;
                void onDisable();
              }}
              disabled={recording}
              style={disableKnobStyle}
            >
              {disableLabel ?? t('settings.shortcuts.disable', 'Disable')}
            </button>
          )}
          <button
            onClick={() => {
              if (disabled) return;
              setRecording(true);
              setError(null);
              resetRecordingState();
            }}
            disabled={recording || disabled}
            style={recordButtonStyle}
          >
            {recording ? t('settings.recording.comboRecordHint') : t('settings.recording.comboRecordBtn')}
          </button>
        </div>
      </div>
      {!comboOnly && presetTriggers.length > 0 && (
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
          <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', alignSelf: 'center' }}>
            {t('settings.recording.modifierPresetsLabel', '常用单键：')}
          </span>
          {presetTriggers.map(trigger => (
            <button
              key={trigger}
              type="button"
              disabled={disabled || recording}
              style={presetChipStyle}
              onClick={() => void finish(shortcutFromLegacyTrigger(trigger))}
            >
              {getHotkeyTriggerLabel(trigger)}
            </button>
          ))}
        </div>
      )}
      {recording && (
        <div
          tabIndex={-1}
          onKeyDown={onKeyDown}
          onKeyUp={onKeyUp}
          style={{ padding: '8px 12px', borderRadius: 8, background: 'rgba(37,99,235,0.06)', border: '1px solid rgba(37,99,235,0.2)', fontSize: 12, color: 'var(--ol-blue)', outline: 'none' }}
          ref={el => el?.focus()}
        >
          {t('settings.recording.comboRecordHint')}
          <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginTop: 4 }}>Esc 取消</div>
        </div>
      )}
      {error && <div style={{ fontSize: 11, color: 'var(--ol-red, #ef4444)' }}>{error}</div>}
    </div>
  );
}

function isModifierKey(key: string): boolean {
  return key === 'Control' || key === 'Alt' || key === 'Shift' || key === 'Meta';
}

function modifierPrimaryFromCode(code: string, key: string): string {
  if (code === 'ShiftLeft') return 'LeftShift';
  if (code === 'ShiftRight') return 'RightShift';
  if (code === 'ControlRight') return 'RightControl';
  if (code === 'ControlLeft') return 'LeftControl';
  if (code === 'AltRight') return 'RightOption';
  if (code === 'AltLeft') return 'LeftOption';
  if (code === 'MetaRight') return 'RightCommand';
  if (code === 'MetaLeft') return 'LeftCommand';
  if (key === 'Shift') return 'Shift';
  return '';
}

function primaryFromKeyboardEvent(e: KeyboardEvent): string {
  const printable = primaryFromPrintableCode(e.code);
  if (printable) return printable;
  if (e.key.length === 1) return e.key;
  const codeToName: Record<string, string> = {
    Space: 'Space',
    Enter: 'Enter',
    Tab: 'Tab',
    Backspace: 'Backspace',
    Delete: 'Delete',
    ArrowUp: 'ArrowUp',
    ArrowDown: 'ArrowDown',
    ArrowLeft: 'ArrowLeft',
    ArrowRight: 'ArrowRight',
    Home: 'Home',
    End: 'End',
    PageUp: 'PageUp',
    PageDown: 'PageDown',
  };
  if (/^F\d{1,2}$/.test(e.key)) return e.key;
  return codeToName[e.code] || e.key;
}

function primaryFromPrintableCode(code: string): string {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  const codeToPrimary: Record<string, string> = {
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
    IntlBackslash: '\\',
  };
  return codeToPrimary[code] || '';
}
