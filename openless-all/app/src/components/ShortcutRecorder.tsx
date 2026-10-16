import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from 'react';
import { Icon } from './Icon';
import { Popover } from '@base-ui/react/popover';
import { Collapse } from './ui/Disclosure';
import { useTranslation } from 'react-i18next';
import {
  chordModifiersFromPressedCodes,
  formatComboParts,
  MODIFIER_CHORD_PRIMARY,
  modifiersFromPressedCodes,
} from '../lib/hotkey';
import {
  primaryFromKeyboardEvent,
  formatShortcutSaveError,
  shortcutFromMouseEvent,
} from '../lib/hotkeyRecorder';
import { KbdGroup } from './Kbd';
import { setShortcutRecordingActive, validateShortcutBinding } from '../lib/ipc';
import type { ShortcutBinding } from '../lib/types';

export function ShortcutRecorder({
  value,
  onSave,
  disabled = false,
  onDisable,
  disableLabel,
  onReset,
  resetLabel,
  comboOnly = false,
  sideSpecificModifiers = false,
  allowMouseButtons = false,
}: {
  value: ShortcutBinding | null;
  onSave: (binding: ShortcutBinding) => Promise<void>;
  disabled?: boolean;
  /** When provided, the menu's Disable is clickable (shortcuts that may be disabled, e.g. QA / style switch). */
  onDisable?: () => void | Promise<void>;
  disableLabel?: string;
  /** When provided, renders Reset in the menu — restores the shortcut's default binding. */
  onReset?: () => void | Promise<void>;
  resetLabel?: string;
  /** Combos only (modifier+key / function key); reject lone modifiers since global hotkeys can't register them. */
  comboOnly?: boolean;
  /** Dictation start/stop only: record side-specific modifiers like cmd-left / ctrl-right. */
  sideSpecificModifiers?: boolean;
  /** Windows dictation only; other shortcut consumers cannot install mouse hooks. */
  allowMouseButtons?: boolean;
}) {
  const { t } = useTranslation();
  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const assigned = Boolean(value?.primary.trim());
  const nativeError =
    error &&
    ['Permission', 'Busy', 'Unavailable', 'Changed'].find((kind) =>
      error.includes(`macDictationKey${kind}`),
    );
  const pendingModifier = useRef<ShortcutBinding | null>(null);
  const pendingTimer = useRef<number | null>(null);
  const pressedCodes = useRef<Set<string>>(new Set());
  const saving = useRef(false);
  const recordingRef = useRef<HTMLDivElement>(null);

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

  useEffect(
    () => () => {
      resetRecordingState();
    },
    [],
  );

  useEffect(() => {
    if (!disabled || !recording) return;
    setRecording(false);
    resetRecordingState();
  }, [disabled, recording]);

  const finish = async (binding: ShortcutBinding) => {
    if (saving.current) return;
    saving.current = true;
    try {
      await validateShortcutBinding(binding);
      await onSave(binding);
      resetRecordingState();
      setRecording(false);
      setError(null);
    } catch (reason) {
      setError(formatShortcutSaveError(reason, t('settings.recording.shortcutSaveFailed')));
    } finally {
      saving.current = false;
    }
  };

  // The browser doesn't deliver Fn keydown: subscribe to the Rust CGEventTap-forwarded
  // event before activating backend recording, so a Fn pressed right as recording
  // starts can't arrive before the listener is registered. Read finish via ref so the
  // effect doesn't re-register on finish identity changes.
  const finishRef = useRef(finish);
  finishRef.current = finish;
  useEffect(() => {
    if (!recording) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const handle = await listen('fn-shortcut-pressed', () => {
          if (cancelled) return;
          void finishRef.current({ primary: 'Fn', modifiers: [] });
        });
        if (cancelled) handle();
        else unlisten = handle;
      } catch (error) {
        console.warn('[shortcut] fn-shortcut-pressed listener failed', error);
      }
      if (cancelled) return;
      try {
        await setShortcutRecordingActive(true);
        if (cancelled) await setShortcutRecordingActive(false);
      } catch (error) {
        console.warn('[shortcut] recording state sync failed', error);
      }
    })();
    const onMouseDown = (e: MouseEvent) => {
      if (cancelled || !allowMouseButtons) return;
      const binding = shortcutFromMouseEvent(e);
      if (!binding) return;
      e.preventDefault();
      e.stopPropagation();
      clearPendingModifier();
      void finishRef.current(binding);
    };
    window.addEventListener('mousedown', onMouseDown, true);
    return () => {
      cancelled = true;
      unlisten?.();
      window.removeEventListener('mousedown', onMouseDown, true);
      void setShortcutRecordingActive(false);
    };
  }, [recording, allowMouseButtons]);

  /** The displayed key is the single recording entry point. */
  const startRecording = () => {
    if (disabled || recording) return;
    setError(null);
    resetRecordingState();
    setRecording(true);
  };

  const onKeyDown = (e: globalThis.KeyboardEvent) => {
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
      if (sideSpecificModifiers) {
        const modifiers = chordModifiersFromPressedCodes(pressedCodes.current);
        if (modifiers.length >= 2) {
          clearPendingModifier();
          const binding = { primary: MODIFIER_CHORD_PRIMARY, modifiers };
          pendingModifier.current = binding;
          pendingTimer.current = window.setTimeout(() => {
            if (pendingModifier.current === binding) {
              void finish(binding);
            }
          }, 650);
          return;
        }
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
      void finish({
        primary,
        modifiers: modifiersFromPressedCodes(pressedCodes.current, sideSpecificModifiers),
      });
    }
  };

  const onKeyUp = (e: globalThis.KeyboardEvent) => {
    if (!recording || disabled || !isModifierKey(e.key)) return;
    e.preventDefault();
    e.stopPropagation();
    pressedCodes.current.delete(e.code);
    if (comboOnly) return;
    if (pendingModifier.current?.primary === MODIFIER_CHORD_PRIMARY) {
      const binding = pendingModifier.current;
      clearPendingModifier();
      void finish(binding);
      return;
    }
    const primary = modifierPrimaryFromCode(e.code, e.key);
    if (primary && pendingModifier.current?.primary === primary) {
      const binding = pendingModifier.current;
      clearPendingModifier();
      void finish(binding);
    }
  };

  const keyHandlers = useRef({ down: onKeyDown, up: onKeyUp });
  keyHandlers.current = { down: onKeyDown, up: onKeyUp };
  useLayoutEffect(() => {
    if (!recording || disabled) return;
    // Shortcut capture cannot depend on focus arriving after a portaled popup
    // opens (or reopens during its exit). Own keys before any surrounding UI.
    const down = (event: globalThis.KeyboardEvent) => keyHandlers.current.down(event);
    const up = (event: globalThis.KeyboardEvent) => keyHandlers.current.up(event);
    window.addEventListener('keydown', down, true);
    window.addEventListener('keyup', up, true);
    return () => {
      window.removeEventListener('keydown', down, true);
      window.removeEventListener('keyup', up, true);
    };
  }, [recording, disabled]);

  const doReset = async () => {
    setError(null);
    try {
      await onReset?.();
    } catch (reason) {
      setError(formatShortcutSaveError(reason, t('settings.recording.shortcutSaveFailed')));
    }
  };

  const doDisable = async () => {
    setError(null);
    try {
      await onDisable?.();
    } catch (reason) {
      setError(formatShortcutSaveError(reason, t('settings.recording.shortcutSaveFailed')));
    }
  };

  const rootStyle: CSSProperties = {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
    width: '100%',
    // The shortcut recorder in settings rows no longer spans the full row (previously
    // the value hugged left and the chevron flew to the far edge); capped to the input
    // width, sitting compactly after the label column.
    maxWidth: 360,
  };
  const recorderRowStyle: CSSProperties = {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    flexWrap: 'wrap',
    width: '100%',
  };
  const menuButtonStyle: CSSProperties = {
    fontSize: 12,
    padding: '5px 12px',
    border: '0.5px solid var(--ol-line-strong)',
    borderRadius: 6,
    background: 'transparent',
    color: 'var(--ol-ink-2)',
    fontFamily: 'inherit',
    fontWeight: 'var(--ol-font-weight-medium)',
    cursor: 'pointer',
    transition: 'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick)',
  };

  return (
    <div style={rootStyle} className="ol-shortcut-recorder">
      <Popover.Root
        open={recording}
        onOpenChange={(next) => {
          if (next) startRecording();
          else {
            setRecording(false);
            setError(null);
            resetRecordingState();
          }
        }}
      >
        <div style={recorderRowStyle}>
          <Popover.Trigger
            className="ol-shortcut-binding"
            disabled={disabled}
            onClick={startRecording}
            aria-label={t('settings.recording.comboRecordBtn')}
          >
            {assigned && value ? <KbdGroup keys={formatComboParts(value)} /> : <span>＋</span>}
          </Popover.Trigger>
          {onReset && (
            <button
              type="button"
              onClick={() => void doReset()}
              disabled={disabled || recording}
              style={menuButtonStyle}
              aria-label={resetLabel ?? t('settings.recording.comboResetBtn')}
              title={resetLabel ?? t('settings.recording.comboResetBtn')}
            >
              <Icon name="refresh" size={13} />
            </button>
          )}
          {onDisable && (
            <button
              type="button"
              onClick={() => void doDisable()}
              disabled={disabled || recording}
              title={disableLabel ?? t('common.clear')}
              aria-label={disableLabel ?? t('common.clear')}
              style={menuButtonStyle}
            >
              <Icon name="close" size={13} />
            </button>
          )}
        </div>
        <Popover.Portal className="ol-popup-portal">
          <Popover.Positioner
            className="ol-select-positioner"
            positionMethod="fixed"
            side="bottom"
            collisionAvoidance={{ side: 'flip', align: 'shift', fallbackAxisSide: 'none' }}
            align="start"
            sideOffset={8}
            collisionPadding={8}
          >
            <Popover.Popup
              className="ol-select-popup ol-shortcut-popup"
              initialFocus={recordingRef}
              aria-label={t('settings.recording.comboRecordBtn')}
            >
              <div tabIndex={-1} ref={recordingRef} className="ol-shortcut-recording">
                {t('settings.recording.comboRecordHint')}
                <small>
                  Esc · {t('common.cancel')}
                  {allowMouseButtons && <> · {t('settings.recording.mouseSideHint')}</>}
                </small>
              </div>
              {error && (
                <div role="alert" style={{ fontSize: 12, color: 'var(--ol-err)' }}>
                  {nativeError ? t(`macDictationKey.${nativeError}`) : error}
                </div>
              )}
            </Popover.Popup>
          </Popover.Positioner>
        </Popover.Portal>
      </Popover.Root>
      <Collapse open={!!error && !recording}>
        <div role="alert" style={{ fontSize: 12, color: 'var(--ol-err)' }}>
          {nativeError ? t(`macDictationKey.${nativeError}`) : error}
        </div>
      </Collapse>
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
