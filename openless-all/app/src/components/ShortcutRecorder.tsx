import { useEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { ChevronDown } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { formatComboParts, modifiersFromPressedCodes } from '../lib/hotkey';
import { KbdGroup } from './Kbd';
import { setShortcutRecordingActive, validateShortcutBinding } from '../lib/ipc';
import type { ShortcutBinding } from '../lib/types';

/** 主行与「正在录入」面板切换时的水平滑动距离（px）。 */
const SLIDE_DISTANCE = 48;
/** 下拉菜单展开后的固定高度（px）：菜单按钮行高恒定，用固定值动画避免每次测量。 */
const MENU_HEIGHT = 34;
/** 滑动切换用 spring（与 Style.tsx 编辑抽屉同款）。只动 transform/opacity，不驱动布局，避免抽搐。 */
const slideSpring = { type: 'spring' as const, damping: 26, stiffness: 280 };
/** 下拉菜单展开/收起缓动，与 --ol-motion-soft 一致。 */
const menuEase = [0.22, 0.8, 0.22, 1] as const;

export function ShortcutRecorder({
  value,
  onSave,
  disabled = false,
  onDisable,
  disableLabel,
  disableDisabled = false,
  disableHint,
  onReset,
  resetLabel,
  comboOnly = false,
  sideSpecificModifiers = false,
}: {
  value: ShortcutBinding | null;
  onSave: (binding: ShortcutBinding) => Promise<void>;
  disabled?: boolean;
  /** 提供则下拉菜单里的「停用」可点（问答/切风格等可停用快捷键）。 */
  onDisable?: () => void | Promise<void>;
  disableLabel?: string;
  /** 置灰「停用」（核心快捷键不可停用），配合 disableHint 展示原因。 */
  disableDisabled?: boolean;
  disableHint?: string;
  /** 提供则下拉菜单里渲染「重置」——恢复该快捷键的默认绑定。 */
  onReset?: () => void | Promise<void>;
  resetLabel?: string;
  /** 仅允许组合键（修饰键+主键 / 功能键）；拒绝单修饰键，因为全局热键无法注册它。 */
  comboOnly?: boolean;
  /** 听写 start/stop 专用：录制 cmd-left / ctrl-right 等侧向修饰键。 */
  sideSpecificModifiers?: boolean;
}) {
  const { t } = useTranslation();
  const [recording, setRecording] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pendingModifier = useRef<ShortcutBinding | null>(null);
  const pendingTimer = useRef<number | null>(null);
  const pressedCodes = useRef<Set<string>>(new Set());
  const rootRef = useRef<HTMLDivElement | null>(null);

  // 菜单打开时：Esc 或点击菜单外部即收起，避免菜单"赖着不关"。
  useEffect(() => {
    if (!menuOpen) return;
    const onKeyDown = (e: globalThis.KeyboardEvent) => {
      if (e.key === 'Escape') setMenuOpen(false);
    };
    const onPointerDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setMenuOpen(false);
    };
    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('mousedown', onPointerDown);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('mousedown', onPointerDown);
    };
  }, [menuOpen]);

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

  /** 开始录入：同时收起菜单——「录制快捷键」按下后，重置/停用两个按钮随之消失。 */
  const startRecording = () => {
    if (disabled || recording) return;
    setMenuOpen(false);
    setError(null);
    resetRecordingState();
    setRecording(true);
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

  const doReset = () => {
    setMenuOpen(false);
    setError(null);
    if (onReset) void onReset();
  };

  const doDisable = () => {
    setMenuOpen(false);
    setError(null);
    if (onDisable) void onDisable();
  };

  // 「停用」可点：有 onDisable 且未被置灰（录音快捷键置灰，核心热键不可停用）。
  const canDisable = Boolean(onDisable) && !disableDisabled;

  const rootStyle: CSSProperties = {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
    width: '100%',
  };
  const recorderRowStyle: CSSProperties = {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    flexWrap: 'wrap',
    width: '100%',
  };
  const chevronButtonStyle: CSSProperties = {
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: 26,
    height: 26,
    padding: 0,
    border: 0,
    borderRadius: 6,
    background: 'transparent',
    color: 'var(--ol-ink-4)',
    fontFamily: 'inherit',
    cursor: 'pointer',
    transition: 'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick)',
  };
  const controlsGroupStyle: CSSProperties = {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 4,
    marginLeft: 'auto',
  };
  const menuButtonStyle: CSSProperties = {
    fontSize: 12,
    padding: '5px 12px',
    border: '0.5px solid var(--ol-line-strong)',
    borderRadius: 6,
    background: 'transparent',
    color: 'var(--ol-ink-2)',
    fontFamily: 'inherit',
    fontWeight: 500,
    cursor: 'pointer',
    transition: 'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick)',
  };
  const menuPrimaryStyle: CSSProperties = {
    ...menuButtonStyle,
    background: 'rgba(37,99,235,0.08)',
    borderColor: 'rgba(37,99,235,0.25)',
    color: 'var(--ol-blue)',
  };
  const disabledMenuButtonStyle: CSSProperties = {
    ...menuButtonStyle,
    opacity: 0.45,
    cursor: 'default',
  };
  const menuRowStyle: CSSProperties = {
    display: 'flex',
    gap: 8,
    paddingTop: 2,
  };

  return (
    <div style={rootStyle} ref={rootRef}>
      {/* mode="wait"：主行与「正在录入」面板不重叠渲染；切换只做 transform/opacity 动画，
          不驱动布局，面板运动过程不抽搐。所有滑入/滑出统一向右。 */}
      <AnimatePresence mode="wait" initial={false}>
        {recording ? (
          <motion.div
            key="recording"
            initial={{ x: SLIDE_DISTANCE, opacity: 0 }}
            animate={{ x: 0, opacity: 1 }}
            exit={{ x: SLIDE_DISTANCE, opacity: 0 }}
            transition={slideSpring}
            tabIndex={-1}
            onKeyDown={onKeyDown}
            onKeyUp={onKeyUp}
            ref={el => el?.focus()}
            style={{
              minHeight: 36,
              display: 'flex',
              flexDirection: 'column',
              justifyContent: 'center',
              padding: '8px 12px',
              borderRadius: 8,
              background: 'rgba(37,99,235,0.06)',
              border: '1px solid rgba(37,99,235,0.2)',
              fontSize: 12,
              color: 'var(--ol-blue)',
              outline: 'none',
            }}
          >
            {t('settings.recording.comboRecordHint')}
            <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginTop: 4 }}>Esc 取消</div>
          </motion.div>
        ) : (
          <motion.div
            key="idle"
            initial={{ x: SLIDE_DISTANCE, opacity: 0 }}
            animate={{ x: 0, opacity: 1 }}
            exit={{ x: SLIDE_DISTANCE, opacity: 0 }}
            transition={slideSpring}
          >
            <div style={recorderRowStyle}>
              {/* 键帽逐键展示（Kbd 组件，用户拍板的快捷键展示标准），替代整块灰底文本。
                  录入入口在展开菜单里（录制快捷键）；主行只留箭头，统一靠最右。 */}
              {value && <KbdGroup keys={formatComboParts(value)} />}
              <div style={controlsGroupStyle}>
                <motion.button
                  whileTap={{ scale: 0.9 }}
                  onClick={() => setMenuOpen(open => !open)}
                  aria-label={t('settings.recording.comboMenuToggle', 'More options')}
                  aria-expanded={menuOpen}
                  title={t('settings.recording.comboMenuToggle', 'More options')}
                  style={chevronButtonStyle}
                >
                  <motion.span
                    animate={{ rotate: menuOpen ? 180 : 0 }}
                    transition={{ duration: 0.16, ease: menuEase }}
                    style={{ display: 'inline-flex' }}
                  >
                    <ChevronDown size={14} strokeWidth={2.2} />
                  </motion.span>
                </motion.button>
              </div>
            </div>
            {/* 下拉菜单：整个板块向下展开，出现 录制快捷键 / 重置 / 停用 三个按钮。
                高度用固定值动画（菜单内容高度恒定），避免 'auto' 每次测量带来的卡顿。 */}
            <AnimatePresence initial={false}>
              {menuOpen && (
                <motion.div
                  key="menu"
                  initial={{ height: 0, opacity: 0 }}
                  animate={{ height: MENU_HEIGHT, opacity: 1 }}
                  exit={{ height: 0, opacity: 0 }}
                  transition={{ duration: 0.16, ease: menuEase }}
                  style={{ overflow: 'hidden' }}
                >
                  <div style={menuRowStyle}>
                    <motion.button
                      initial={{ y: 4, opacity: 0 }}
                      animate={{ y: 0, opacity: 1 }}
                      transition={{ duration: 0.16, ease: menuEase }}
                      whileHover={{ y: -1 }}
                      whileTap={{ scale: 0.96 }}
                      onClick={startRecording}
                      style={menuPrimaryStyle}
                    >
                      {t('settings.recording.comboRecordBtn')}
                    </motion.button>
                    {onReset && (
                      <motion.button
                        initial={{ y: 4, opacity: 0 }}
                        animate={{ y: 0, opacity: 1 }}
                        transition={{ duration: 0.16, ease: menuEase, delay: 0.03 }}
                        whileHover={{ y: -1 }}
                        whileTap={{ scale: 0.96 }}
                        onClick={doReset}
                        style={menuButtonStyle}
                      >
                        {resetLabel ?? t('settings.recording.comboResetBtn', 'Reset')}
                      </motion.button>
                    )}
                    <motion.button
                      initial={{ y: 4, opacity: 0 }}
                      animate={{ y: 0, opacity: 1 }}
                      transition={{ duration: 0.16, ease: menuEase, delay: 0.06 }}
                      whileHover={canDisable ? { y: -1 } : undefined}
                      whileTap={canDisable ? { scale: 0.96 } : undefined}
                      onClick={canDisable ? doDisable : undefined}
                      title={canDisable ? undefined : disableHint}
                      style={canDisable ? menuButtonStyle : disabledMenuButtonStyle}
                    >
                      {disableLabel ?? t('settings.shortcuts.disable', 'Disable')}
                    </motion.button>
                  </div>
                </motion.div>
              )}
            </AnimatePresence>
          </motion.div>
        )}
      </AnimatePresence>
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
