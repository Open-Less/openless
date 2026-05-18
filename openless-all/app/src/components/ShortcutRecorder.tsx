import { useEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { currentPlatform, formatComboLabel } from '../lib/hotkey';
import { setShortcutRecordingActive, validateShortcutBinding } from '../lib/ipc';
import type { ShortcutBinding } from '../lib/types';

export function ShortcutRecorder({
  value,
  onSave,
  alignRecordButton = false,
  disabled = false,
  allowUnset = false,
}: {
  value: ShortcutBinding;
  onSave: (binding: ShortcutBinding) => Promise<void>;
  alignRecordButton?: boolean;
  disabled?: boolean;
  allowUnset?: boolean;
}) {
  const { t } = useTranslation();
  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pendingModifier = useRef<ShortcutBinding | null>(null);
  const pendingTimer = useRef<number | null>(null);

  const clearPendingModifier = () => {
    if (pendingTimer.current !== null) {
      window.clearTimeout(pendingTimer.current);
      pendingTimer.current = null;
    }
    pendingModifier.current = null;
  };

  useEffect(() => () => {
    clearPendingModifier();
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
    clearPendingModifier();
  }, [disabled, recording]);

  const finish = async (binding: ShortcutBinding) => {
    try {
      await validateShortcutBinding(binding);
      await onSave(binding);
      clearPendingModifier();
      setRecording(false);
      setError(null);
    } catch {
      setError(t('settings.recording.comboConflict'));
    }
  };

  const clearShortcut = async () => {
    if (disabled) return;
    try {
      await onSave({ primary: '', modifiers: [] });
      clearPendingModifier();
      setRecording(false);
      setError(null);
    } catch {
      setError(t('common.operationFailed'));
    }
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (!recording || disabled) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') {
      setRecording(false);
      setError(null);
      clearPendingModifier();
      return;
    }
    if (isModifierKey(e.key)) {
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
    if (primary) void finish({ primary, modifiers: modifiersFromKeyboardEvent(e) });
  };

  const onKeyUp = (e: KeyboardEvent<HTMLDivElement>) => {
    if (!recording || disabled || !isModifierKey(e.key)) return;
    e.preventDefault();
    e.stopPropagation();
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
    marginLeft: alignRecordButton ? 'auto' : undefined,
  };
  const clearButtonStyle: CSSProperties = {
    fontSize: 12,
    padding: '5px 10px',
    background: 'transparent',
    color: 'var(--ol-ink-3)',
    border: '0.5px solid var(--ol-line-strong)',
    borderRadius: 6,
    fontFamily: 'inherit',
    fontWeight: 500,
    cursor: disabled ? 'default' : 'pointer',
    opacity: disabled || !value.primary.trim() ? 0.55 : 1,
  };
  const displayLabel = value.primary.trim()
    ? formatComboLabel(value)
    : t('hotkey.unset', 'Unset');

  return (
    <div style={rootStyle}>
      <div style={recorderRowStyle}>
        <span style={{ padding: '4px 10px', borderRadius: 6, background: 'rgba(0,0,0,0.06)', fontSize: 13, fontFamily: 'var(--ol-font-mono)', fontWeight: 500, color: 'var(--ol-ink)' }}>
          {displayLabel}
        </span>
        <button
          onClick={() => {
            if (disabled) return;
            setRecording(true);
            setError(null);
            clearPendingModifier();
          }}
          disabled={recording || disabled}
          style={recordButtonStyle}
        >
          {recording ? t('settings.recording.comboRecordHint') : t('settings.recording.comboRecordBtn')}
        </button>
        {allowUnset && Boolean(value.primary.trim()) && (
          <button
            onClick={() => void clearShortcut()}
            disabled={disabled}
            style={clearButtonStyle}
          >
            {t('settings.advanced.disable', 'Disable')}
          </button>
        )}
      </div>
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

function modifiersFromKeyboardEvent(e: KeyboardEvent): string[] {
  const modifiers: string[] = [];
  if (e.metaKey && e.key !== 'Meta') modifiers.push(currentPlatform().isMac ? 'cmd' : 'super');
  if (e.ctrlKey && e.key !== 'Control') modifiers.push('ctrl');
  if (e.altKey && e.key !== 'Alt') modifiers.push('alt');
  if (e.shiftKey && e.key !== 'Shift') modifiers.push('shift');
  return modifiers;
}

function isModifierKey(key: string): boolean {
  return key === 'Control' || key === 'Alt' || key === 'Shift' || key === 'Meta';
}

function modifierPrimaryFromCode(code: string, key: string): string {
  if (key === 'Shift') return 'Shift';
  if (code === 'ControlRight') return 'RightControl';
  if (code === 'ControlLeft') return 'LeftControl';
  if (code === 'AltRight') return 'RightOption';
  if (code === 'AltLeft') return 'LeftOption';
  if (code === 'MetaRight' || code === 'MetaLeft') return 'RightCommand';
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
