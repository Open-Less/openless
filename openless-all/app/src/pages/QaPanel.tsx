// QaPanel.tsx — 划词追问浮窗 v2（issue #118 v2）。
//
// 触发链路（v2 双 hotkey）：
//   1) 用户按 Cmd+Shift+;（默认）→ 后端 toggle 浮窗可见性。
//      显示时发 `qa:state { kind: "idle", messages: [] }`。
//   2) 浮窗可见时，用户按 rightOption（主听写键的复用）→ 录音；
//      再按一次 → ASR + LLM；后端推 `qa:state { kind: "answer", messages: [...] }`。
//   3) 答案后用户可继续按 Option 多轮提问，messages 累积。
//
// 关闭：Esc / Close 按钮 / 再按 Cmd+Shift+; → qa_window_dismiss → 后端清历史 + 隐藏窗口。
// **不再自动关**（v1 的 blur / 30s 超时去掉）：用户多轮思考时浮窗保持。

import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import {
  getPlatformCapabilities,
  getSettings,
  isTauri,
  qaSubmitText,
  qaToggleRecording,
  qaWindowDismiss,
  qaWindowPin,
} from '../lib/ipc';
import type { PlatformCapabilities, QaChatMessage, QaStatePayload, UserPreferences } from '../lib/types';
import { getHotkeyBindingLabel } from '../lib/hotkey';
import { renderQaMarkdown, renderQaPlainText } from '../lib/qaMarkdown';

const SELECTION_PREVIEW_MAX = 60;

type Status = 'idle' | 'recording' | 'thinking' | 'error';

interface QaPanelProps {
  embedded?: boolean;
  onRequestClose?: () => void;
}

export function QaPanel({ embedded = false, onRequestClose }: QaPanelProps = {}) {
  const { t, i18n } = useTranslation();
  const [messages, setMessages] = useState<QaChatMessage[]>([]);
  const [status, setStatus] = useState<Status>('idle');
  const [errorMsg, setErrorMsg] = useState<string>('');
  const [selectionPreview, setSelectionPreview] = useState<string>('');
  const [composerText, setComposerText] = useState<string>('');
  const [pinned, setPinned] = useState(false);
  /** 流式 LLM 答案：answer_delta 累积、answer 事件来时清空（最终内容已落到 messages）。 */
  const [streamingAnswer, setStreamingAnswer] = useState<string>('');
  /** 录音电平：0..1。后端每帧 33ms 通过 qa:level emit。详见 issue #162。 */
  const [level, setLevel] = useState<number>(0);
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);
  /** 用户当前的录音热键 label（如 "右 Option" / "Right Alt"）。issue #205：
   *  原版硬编码 "Option"，Windows 用户没这个键，文案失真。读 prefs 后由 i18n
   *  插值动态显示，平台与用户配置都能跟上。 */
  const [recordHotkeyLabel, setRecordHotkeyLabel] = useState<string>(() =>
    i18n.t('hotkey.fallback'),
  );
  const tRef = useRef(t);
  tRef.current = t;
  const embeddedRef = useRef(embedded);
  embeddedRef.current = embedded;
  const onRequestCloseRef = useRef(onRequestClose);
  onRequestCloseRef.current = onRequestClose;

  // ── 后端事件订阅（mount 时订阅一次，永不重订阅）──────────────────
  useEffect(() => {
    if (!isTauri) return;
    let unlistenState: (() => void) | undefined;
    let unlistenDismiss: (() => void) | undefined;
    let unlistenLevel: (() => void) | undefined;
    let unlistenPrefs: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const stateHandle = await listen<QaStatePayload>('qa:state', event => {
          const payload = event.payload;
          if (payload.messages) {
            setMessages(payload.messages);
          }
          switch (payload.kind) {
            case 'idle':
              setStatus('idle');
              setSelectionPreview('');
              setErrorMsg('');
              setStreamingAnswer('');
              setLevel(0);
              break;
            case 'recording':
              setStatus('recording');
              setSelectionPreview(payload.selection_preview ?? '');
              setErrorMsg('');
              setStreamingAnswer('');
              break;
            case 'loading':
              // ASR 在 finalize、user message 还没 push 的过渡帧。提前切到 thinking
              // 视图避免 UI 卡 recording 几百 ms 反馈缺失。详见 issue #161。
              setStatus('thinking');
              if (payload.selection_preview != null) {
                setSelectionPreview(payload.selection_preview);
              }
              setErrorMsg('');
              setStreamingAnswer('');
              setLevel(0);
              break;
            case 'thinking':
              setStatus('thinking');
              if (payload.selection_preview != null) {
                setSelectionPreview(payload.selection_preview);
              }
              setErrorMsg('');
              setStreamingAnswer('');
              setLevel(0);
              break;
            case 'answer_delta':
              // 流式增量。仍保持 thinking 状态——直到 answer 事件落定后才回 idle。
              if (payload.chunk) {
                setStreamingAnswer(prev => prev + payload.chunk);
              }
              break;
            case 'answer':
              setStatus('idle');
              setErrorMsg('');
              // messages 已被上面的 setMessages 落定，清掉流式 buffer 避免和最终气泡重影。
              setStreamingAnswer('');
              setLevel(0);
              break;
            case 'error':
              setStatus('error');
              setErrorMsg(payload.error ?? tRef.current('qa.error'));
              setStreamingAnswer('');
              setLevel(0);
              break;
          }
        });
        const dismissHandle = await listen<unknown>('qa:dismiss', () => {
          setPinned(false);
          setSelectionPreview('');
          setComposerText('');
          if (embeddedRef.current) {
            onRequestCloseRef.current?.();
          } else {
            void qaWindowDismiss();
          }
        });
        // qa:level — 录音电平，节流 ~33ms/帧。详见 issue #162。
        const levelHandle = await listen<{ level: number }>('qa:level', event => {
          setLevel(event.payload.level ?? 0);
        });
        // prefs:changed — 后端在 set_settings 后广播。issue #205：QA 浮窗在独立
        // webview，没有 HotkeySettingsContext；如果用户在主窗口改了录音键，
        // 浮窗里的 "{recordHotkey}" 文案必须立刻跟上，否则会一直停在旧值。
        const prefsHandle = await listen<UserPreferences>('prefs:changed', event => {
          setRecordHotkeyLabel(getHotkeyBindingLabel(event.payload?.hotkey));
        });
        if (cancelled) {
          stateHandle();
          dismissHandle();
          levelHandle();
          prefsHandle();
        } else {
          unlistenState = stateHandle;
          unlistenDismiss = dismissHandle;
          unlistenLevel = levelHandle;
          unlistenPrefs = prefsHandle;
        }
      } catch (error) {
        console.error('[QaPanel] listener setup failed', error);
      }
    })();
    return () => {
      cancelled = true;
      unlistenState?.();
      unlistenDismiss?.();
      unlistenLevel?.();
      unlistenPrefs?.();
    };
  }, []);

  // ── Esc 关闭 ────────────────────────────────────────────────────────
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        void qaWindowDismiss();
        onRequestClose?.();
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [onRequestClose]);

  // ── 读取用户当前的录音热键 label，给 i18n 插值用（issue #205）。
  // QaPanel 跑在独立 webview（label="qa"），没有 HotkeySettingsContext
  // 注入，所以直接走 IPC 拿一次 prefs。语言切换时 i18n.t('hotkey.fallback')
  // 也要跟着重算，所以 i18n.language 入依赖。
  useEffect(() => {
    if (!isTauri) {
      setRecordHotkeyLabel(i18n.t('hotkey.fallback'));
      return;
    }
    let cancelled = false;
    void getSettings()
      .then(prefs => {
        if (cancelled) return;
        setRecordHotkeyLabel(getHotkeyBindingLabel(prefs.hotkey));
      })
      .catch(err => {
        console.warn('[QaPanel] load hotkey label failed', err);
      });
    return () => {
      cancelled = true;
    };
  }, [i18n.language]);

  useEffect(() => {
    let cancelled = false;
    void getPlatformCapabilities()
      .then(caps => {
        if (!cancelled) setPlatformCaps(caps);
      })
      .catch(err => {
        console.warn('[QaPanel] load platform capabilities failed', err);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const mobileRecordButton = platformCaps?.supportsDesktopHotkey === false;
  const recordControlLabel = mobileRecordButton
    ? t('qa.mobileRecordLabel')
    : recordHotkeyLabel;

  const onTogglePin = () => {
    const next = !pinned;
    setPinned(next);
    void qaWindowPin(next);
  };

  const onClose = () => {
    void qaWindowDismiss();
    onRequestClose?.();
  };

  const onSubmitText = () => {
    const text = composerText.trim();
    if (!text || status === 'thinking' || status === 'recording') return;
    setComposerText('');
    void qaSubmitText(text).catch(error => {
      console.error('[QaPanel] qa_submit_text failed', error);
      setErrorMsg(error instanceof Error ? error.message : String(error));
      setStatus('error');
    });
  };

  // ── 自动滚动到底（新消息进来时）────────────────────────────────────
  const scrollRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [messages, status]);

  return (
    <div className="ol-frost" style={embedded ? embeddedShellStyle : shellStyle}>
      <Toolbar pinned={pinned} onTogglePin={onTogglePin} onClose={onClose} embedded={embedded} />
      <div ref={scrollRef} style={contentStyle}>
        {messages.length === 0 && status === 'idle' && (
          <EmptyHint t={t} recordHotkey={recordControlLabel} />
        )}
        {messages.length === 0 && status === 'recording' && (
          <RecordingHeader
            preview={selectionPreview}
            t={t}
            level={level}
            recordHotkey={recordControlLabel}
          />
        )}
        <MessageList messages={messages} />
        {status === 'recording' && messages.length > 0 && (
          <TurnIndicator
            kind="recording"
            t={t}
            preview={selectionPreview}
            level={level}
            recordHotkey={recordControlLabel}
          />
        )}
        {streamingAnswer && (
          <StreamingAssistantBubble markdown={streamingAnswer} />
        )}
        {status === 'thinking' && !streamingAnswer && (
          <TurnIndicator kind="thinking" t={t} />
        )}
        {status === 'error' && (
          <ErrorRow message={errorMsg} t={t} recordHotkey={recordControlLabel} />
        )}
      </div>
      {mobileRecordButton && (
        <MobileRecordButton
          status={status}
          t={t}
          onClick={() => {
            if (status === 'thinking') return;
            void qaToggleRecording();
          }}
        />
      )}
      <QaComposer
        value={composerText}
        disabled={status === 'thinking' || status === 'recording'}
        t={t}
        onChange={setComposerText}
        onSubmit={onSubmitText}
      />
      <StatusBar status={status} t={t} recordHotkey={recordControlLabel} />
    </div>
  );
}

// ── 子组件 ────────────────────────────────────────────────────────────

interface ToolbarProps {
  pinned: boolean;
  onTogglePin: () => void;
  onClose: () => void;
  embedded?: boolean;
}

function Toolbar({ pinned, onTogglePin, onClose, embedded = false }: ToolbarProps) {
  const { t } = useTranslation();
  // 拖动 (issue #205)：
  // - macOS: lib.rs::make_qa_window_draggable_macos 在 NSWindow 层把整窗口设
  //   movableByWindowBackground=YES，所以 macOS 上整片背景都可拖。
  // - Windows: NSWindow 走不了；用 Tauri 标准 data-tauri-drag-region —— mousedown
  //   走 startDragging() → WM_NCLBUTTONDOWN(HTCAPTION)，在 focus:false 浮窗上也能用。
  // 两条路径并存不冲突；data-tauri-drag-region 放在 toolbar 的空白 spacer 上，IconBtn
  // 作为 button 子元素仍然正常 click。
  return (
    <div style={embedded ? embeddedToolbarStyle : toolbarStyle}>
      <div data-tauri-drag-region style={{ flex: 1, height: '100%' }} />
      <IconBtn
        label={pinned ? t('qa.unpinTooltip') : t('qa.pinTooltip')}
        active={pinned}
        onClick={onTogglePin}
      >
        <svg width="13" height="13" viewBox="0 0 16 16" fill="none">
          <path
            d="M10.5 2L14 5.5L11.5 8L9.5 7L7 9.5L6.5 9L4 11.5L3 13L4.5 11.5L7 9L6.5 8.5L9 6L8 4L10.5 2Z"
            stroke="currentColor"
            strokeWidth="1.2"
            strokeLinejoin="round"
            fill={pinned ? 'currentColor' : 'none'}
          />
        </svg>
      </IconBtn>
      <IconBtn label={t('qa.closeTooltip')} onClick={onClose}>
        <svg width="11" height="11" viewBox="0 0 11 11">
          <path
            d="M1.5 1.5l8 8M9.5 1.5l-8 8"
            stroke="currentColor"
            strokeWidth="1.6"
            strokeLinecap="round"
          />
        </svg>
      </IconBtn>
    </div>
  );
}

interface IconBtnProps {
  label: string;
  active?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}

function IconBtn({ label, active, onClick, children }: IconBtnProps) {
  return (
    <button
      onClick={onClick}
      title={label}
      aria-label={label}
      style={{
        ...iconBtnBaseStyle,
        color: active ? 'var(--ol-blue)' : 'var(--ol-ink-3)',
        background: active ? 'rgba(37,99,235,0.12)' : 'transparent',
      }}
    >
      {children}
    </button>
  );
}

function EmptyHint({
  t,
  recordHotkey,
}: {
  t: ReturnType<typeof useTranslation>['t'];
  recordHotkey: string;
}) {
  return (
    <div style={emptyHintStyle}>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 6, color: 'var(--ol-ink)' }}>
        {t('qa.emptyTitle', { recordHotkey })}
      </div>
      <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.6 }}>
        {t('qa.emptyDesc', { recordHotkey })}
      </div>
    </div>
  );
}

function RecordingHeader({
  preview,
  t,
  level,
  recordHotkey,
}: {
  preview: string;
  t: ReturnType<typeof useTranslation>['t'];
  level: number;
  recordHotkey: string;
}) {
  const truncated = useMemo(() => truncate(preview, SELECTION_PREVIEW_MAX), [preview]);
  return (
    <div style={recordingHeaderStyle}>
      {truncated && (
        <div style={previewStyle}>
          <span style={{ color: 'var(--ol-ink-4)', marginRight: 4 }}>
            {t('qa.selectionPreview')}
          </span>
          <span style={{ color: 'var(--ol-ink-2)' }}>{truncated}</span>
        </div>
      )}
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 12, color: 'var(--ol-ink-2)' }}>
        <span style={recordingDotStyle} />
        {t('qa.recordingHint', { recordHotkey })}
      </div>
      <LevelBar level={level} />
    </div>
  );
}

/** QA 录音电平条。后端 qa:level 每帧 ~33ms 推一次 0..1。详见 issue #162。 */
function LevelBar({ level }: { level: number }) {
  const pct = Math.min(100, Math.max(0, level * 100));
  return (
    <div
      style={{
        height: 4,
        width: '100%',
        background: 'rgba(0,0,0,0.06)',
        borderRadius: 2,
        overflow: 'hidden',
      }}
    >
      <div
        style={{
          height: '100%',
          width: `${pct}%`,
          background: 'var(--ol-blue)',
          transition: 'width 0.08s var(--ol-motion-quick)',
        }}
      />
    </div>
  );
}

function MessageList({ messages }: { messages: QaChatMessage[] }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      {messages.map((m, i) => (
        <MessageRow key={i} message={m} />
      ))}
    </div>
  );
}

function MessageRow({ message }: { message: QaChatMessage }) {
  // 钩子顺序与 message.role 无关：先无条件 useMemo（user 消息时 html 不渲染但计算无害）。
  const html = useMemo(() => {
    if (message.role !== 'assistant') return '';
    try {
      return renderQaMarkdown(message.content);
    } catch (error) {
      console.error('[qa] failed to render markdown', error);
      return renderQaPlainText(String(message.content ?? ''));
    }
  }, [message.content, message.role]);

  if (message.role === 'user') {
    // 第一轮可能含 "# 选区原文 ... # 我的问题 ..." → 抽出问题部分单独显示，
    // 选区作为引用块淡显在上面。
    const { selection, question } = splitFirstTurnUser(message.content);
    return (
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-end', gap: 4 }}>
        {selection && (
          <div style={selectionQuoteStyle}>
            <span style={{ color: 'var(--ol-ink-4)', marginRight: 4 }}>“</span>
            {truncate(selection, 120)}
            <span style={{ color: 'var(--ol-ink-4)', marginLeft: 4 }}>”</span>
          </div>
        )}
        <div style={userBubbleStyle}>{question}</div>
      </div>
    );
  }
  return (
    <div
      className="qa-answer"
      style={assistantBubbleStyle}
      // eslint-disable-next-line react/no-danger
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

/** 流式 LLM 答案的 in-progress 气泡。跟 assistant 最终气泡同款样式，结尾加一颗
 *  闪烁的 caret 让用户看出还在生成。markdown 边到边渲染，未闭合的代码块不会炸 —
 *  marked 在不完整输入上是宽容的（开 token 没找到闭 token 就当 inline）。 */
function StreamingAssistantBubble({ markdown }: { markdown: string }) {
  const html = useMemo(() => {
    try {
      return renderQaMarkdown(markdown);
    } catch (error) {
      console.error('[qa] failed to render streaming markdown', error);
      return renderQaPlainText(String(markdown ?? ''));
    }
  }, [markdown]);
  return (
    <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-start', gap: 4 }}>
      <div
        className="qa-answer"
        style={assistantBubbleStyle}
        // eslint-disable-next-line react/no-danger
        dangerouslySetInnerHTML={{ __html: html }}
      />
      <span
        style={{
          display: 'inline-block',
          width: 6,
          height: 12,
          background: 'var(--ol-blue)',
          marginLeft: 12,
          animation: 'qa-pulse 0.9s var(--ol-motion-soft) infinite',
          borderRadius: 1,
        }}
      />
    </div>
  );
}

function splitFirstTurnUser(content: string): { selection: string; question: string } {
  // 后端拼法：`# 选区原文\n{sel}\n\n# 我的问题\n{q}`。简单 split，对齐 coordinator.rs 的写法。
  const m = content.match(/^# 选区原文\n([\s\S]*?)\n\n# 我的问题\n([\s\S]+)$/);
  if (!m) return { selection: '', question: content };
  return { selection: m[1].trim(), question: m[2].trim() };
}

function TurnIndicator({
  kind,
  preview,
  t,
  level,
  recordHotkey,
}: {
  kind: 'recording' | 'thinking';
  preview?: string;
  t: ReturnType<typeof useTranslation>['t'];
  level?: number;
  recordHotkey?: string;
}) {
  if (kind === 'recording') {
    const truncated = preview ? truncate(preview, SELECTION_PREVIEW_MAX) : '';
    return (
      <div style={turnIndicatorStyle}>
        {truncated && (
          <div style={previewInlineStyle}>
            <span style={{ color: 'var(--ol-ink-4)', marginRight: 4 }}>
              {t('qa.selectionPreview')}
            </span>
            <span style={{ color: 'var(--ol-ink-2)' }}>{truncated}</span>
          </div>
        )}
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 12, color: 'var(--ol-ink-2)' }}>
          <span style={recordingDotStyle} />
          {t('qa.recordingHint', { recordHotkey: recordHotkey ?? '' })}
        </div>
        <LevelBar level={level ?? 0} />
      </div>
    );
  }
  return (
    <div style={turnIndicatorStyle}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 12, color: 'var(--ol-ink-3)' }}>
        <SkeletonLine width="60%" />
      </div>
      <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', fontWeight: 500 }}>
        {t('qa.thinking')}
      </div>
    </div>
  );
}

function ErrorRow({
  message,
  t,
  recordHotkey,
}: {
  message: string;
  t: ReturnType<typeof useTranslation>['t'];
  recordHotkey: string;
}) {
  return (
    <div style={errorRowStyle}>
      <div style={{ fontSize: 12.5, color: 'var(--ol-err)', lineHeight: 1.55 }}>{message}</div>
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)' }}>
        {t('qa.errorRetryHint', { recordHotkey })}
      </div>
    </div>
  );
}

function StatusBar({
  status,
  t,
  recordHotkey,
}: {
  status: Status;
  t: ReturnType<typeof useTranslation>['t'];
  recordHotkey: string;
}) {
  let label = '';
  let dotColor = 'transparent';
  switch (status) {
    case 'idle':
      label = t('qa.statusIdle', { recordHotkey });
      dotColor = 'rgba(0,0,0,0.18)';
      break;
    case 'recording':
      label = t('qa.statusRecording');
      dotColor = 'var(--ol-err)';
      break;
    case 'thinking':
      label = t('qa.statusThinking');
      dotColor = 'var(--ol-blue)';
      break;
    case 'error':
      label = t('qa.statusError');
      dotColor = 'var(--ol-err)';
      break;
  }
  return (
    <div style={statusBarStyle}>
      <span
        style={{
          width: 7,
          height: 7,
          borderRadius: '50%',
          background: dotColor,
          flexShrink: 0,
        }}
      />
      <span style={{ fontSize: 11.5, color: 'var(--ol-ink-3)', fontWeight: 500 }}>{label}</span>
    </div>
  );
}

function MobileRecordButton({
  status,
  t,
  onClick,
}: {
  status: Status;
  t: ReturnType<typeof useTranslation>['t'];
  onClick: () => void;
}) {
  const disabled = status === 'thinking';
  const recording = status === 'recording';
  return (
    <div style={mobileRecordDockStyle}>
      <button
        type="button"
        onClick={onClick}
        disabled={disabled}
        style={{
          ...mobileRecordButtonStyle,
          background: recording ? 'var(--ol-err)' : 'var(--ol-blue)',
          opacity: disabled ? 0.48 : 1,
        }}
      >
        {recording ? t('qa.mobileRecordStop') : t('qa.mobileRecordStart')}
      </button>
    </div>
  );
}

function QaComposer({
  value,
  disabled,
  t,
  onChange,
  onSubmit,
}: {
  value: string;
  disabled: boolean;
  t: ReturnType<typeof useTranslation>['t'];
  onChange: (value: string) => void;
  onSubmit: () => void;
}) {
  const canSubmit = value.trim().length > 0 && !disabled;
  return (
    <div style={composerStyle}>
      <textarea
        value={value}
        disabled={disabled}
        rows={2}
        placeholder={t('qa.composerPlaceholder')}
        onChange={event => onChange(event.currentTarget.value)}
        onKeyDown={event => {
          if (event.key === 'Enter' && !event.shiftKey) {
            event.preventDefault();
            if (canSubmit) onSubmit();
          }
        }}
        style={composerTextareaStyle}
      />
      <button
        type="button"
        disabled={!canSubmit}
        onClick={onSubmit}
        aria-label={t('qa.composerSend')}
        title={t('qa.composerSend')}
        style={{
          ...composerSendStyle,
          opacity: canSubmit ? 1 : 0.45,
          cursor: canSubmit ? 'pointer' : 'default',
        }}
      >
        {t('qa.composerSend')}
      </button>
    </div>
  );
}

function SkeletonLine({ width }: { width: string }) {
  return (
    <div
      style={{
        height: 8,
        width,
        borderRadius: 4,
        background:
          'linear-gradient(90deg, rgba(0,0,0,0.06) 0%, rgba(0,0,0,0.12) 50%, rgba(0,0,0,0.06) 100%)',
        backgroundSize: '200% 100%',
        animation: 'qa-skeleton 1.4s var(--ol-motion-soft) infinite',
      }}
    />
  );
}

function truncate(text: string, max: number): string {
  if (text.length <= max) return text;
  return `${text.slice(0, max)}…`;
}

// ── 样式 ──────────────────────────────────────────────────────────────

// 非 Linux 走 .ol-frost 假毛玻璃；Linux 禁用透明窗口后退成不透明面。
// 这里只管布局 + 内描边高光 + 柔和阴影，所以不写 background / backdrop-filter。
const shellStyle: CSSProperties = {
  width: '100%',
  height: '100vh',
  display: 'flex',
  flexDirection: 'column',
  borderRadius: 14,
  overflow: 'hidden',
  border: '0.5px solid rgba(0, 0, 0, 0.08)',
  boxShadow: 'var(--ol-shadow-lg), inset 0 1px 0 0 rgba(255, 255, 255, 0.9)',
  fontFamily: 'var(--ol-font-sans)',
  color: 'var(--ol-ink)',
};

const embeddedShellStyle: CSSProperties = {
  ...shellStyle,
  borderRadius: 0,
  border: 0,
  boxShadow: 'none',
  minHeight: '100vh',
};

const toolbarStyle: CSSProperties = {
  height: 32,
  display: 'flex',
  alignItems: 'center',
  gap: 4,
  padding: '0 8px',
  borderBottom: '0.5px solid rgba(0, 0, 0, 0.06)',
  flexShrink: 0,
  cursor: 'grab',
};

const iconBtnBaseStyle: CSSProperties = {
  width: 22,
  height: 22,
  border: 0,
  borderRadius: 6,
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  cursor: 'default',
  padding: 0,
  transition: 'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick)',
};

const contentStyle: CSSProperties = {
  flex: 1,
  minHeight: 0,
  overflow: 'auto',
  padding: 16,
  display: 'flex',
  flexDirection: 'column',
  gap: 12,
};

const emptyHintStyle: CSSProperties = {
  margin: 'auto 0',
  textAlign: 'center',
  padding: '0 8px',
};

const recordingHeaderStyle: CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 8,
};

const previewStyle: CSSProperties = {
  fontSize: 11.5,
  lineHeight: 1.5,
  padding: '8px 10px',
  borderRadius: 8,
  background: 'rgba(0, 0, 0, 0.035)',
  border: '0.5px solid rgba(0, 0, 0, 0.06)',
};

const previewInlineStyle: CSSProperties = {
  ...previewStyle,
  marginBottom: 4,
};

const turnIndicatorStyle: CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 6,
};

const userBubbleStyle: CSSProperties = {
  maxWidth: '80%',
  padding: '8px 12px',
  borderRadius: 14,
  borderBottomRightRadius: 4,
  background: 'var(--ol-blue)',
  color: '#fff',
  fontSize: 13,
  lineHeight: 1.55,
  wordBreak: 'break-word',
};

const selectionQuoteStyle: CSSProperties = {
  maxWidth: '80%',
  padding: '6px 10px',
  borderRadius: 10,
  background: 'rgba(0,0,0,0.04)',
  border: '0.5px solid rgba(0,0,0,0.06)',
  fontSize: 11.5,
  color: 'var(--ol-ink-3)',
  fontStyle: 'italic',
  lineHeight: 1.5,
};

const assistantBubbleStyle: CSSProperties = {
  maxWidth: '92%',
  padding: '8px 12px',
  borderRadius: 14,
  borderBottomLeftRadius: 4,
  background: 'rgba(0,0,0,0.04)',
  fontSize: 13,
  lineHeight: 1.6,
  color: 'var(--ol-ink)',
  wordBreak: 'break-word',
  alignSelf: 'flex-start',
};

const errorRowStyle: CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 4,
  padding: '8px 12px',
  borderRadius: 10,
  background: 'rgba(220,38,38,0.06)',
  border: '0.5px solid rgba(220,38,38,0.18)',
};

const recordingDotStyle: CSSProperties = {
  width: 8,
  height: 8,
  borderRadius: '50%',
  background: 'var(--ol-err)',
  animation: 'qa-pulse 1.2s var(--ol-motion-soft) infinite',
};

const statusBarStyle: CSSProperties = {
  height: 28,
  flexShrink: 0,
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  padding: '0 16px',
  borderTop: '0.5px solid rgba(0, 0, 0, 0.06)',
  background: 'rgba(255,255,255,0.4)',
};

const embeddedToolbarStyle: CSSProperties = {
  ...toolbarStyle,
  height: 'calc(44px + env(safe-area-inset-top, 0px))',
  padding: 'env(safe-area-inset-top, 0px) 14px 0 14px',
  cursor: 'default',
};

const mobileRecordDockStyle: CSSProperties = {
  flexShrink: 0,
  padding: '10px 16px 0',
  display: 'flex',
};

const mobileRecordButtonStyle: CSSProperties = {
  width: '100%',
  minHeight: 42,
  border: 0,
  borderRadius: 10,
  color: '#fff',
  fontSize: 14,
  fontWeight: 700,
  fontFamily: 'var(--ol-font-sans)',
};

const composerStyle: CSSProperties = {
  flexShrink: 0,
  display: 'grid',
  gridTemplateColumns: 'minmax(0, 1fr) auto',
  alignItems: 'end',
  gap: 8,
  padding: '10px 12px',
  borderTop: '0.5px solid rgba(0, 0, 0, 0.06)',
  background: 'rgba(255,255,255,0.48)',
};

const composerTextareaStyle: CSSProperties = {
  width: '100%',
  minHeight: 40,
  maxHeight: 88,
  resize: 'vertical',
  border: '0.5px solid rgba(0,0,0,0.12)',
  borderRadius: 8,
  padding: '8px 9px',
  fontFamily: 'var(--ol-font-sans)',
  fontSize: 13,
  lineHeight: 1.45,
  color: 'var(--ol-ink)',
  background: 'rgba(255,255,255,0.78)',
  outline: 'none',
};

const composerSendStyle: CSSProperties = {
  minHeight: 40,
  minWidth: 58,
  border: 0,
  borderRadius: 8,
  padding: '0 12px',
  background: 'var(--ol-blue)',
  color: '#fff',
  fontFamily: 'var(--ol-font-sans)',
  fontSize: 13,
  fontWeight: 700,
};

const globalCss = `
@keyframes qa-skeleton {
  0%   { background-position: 200% 0; filter: blur(.2px); }
  50%  { filter: blur(0); }
  100% { background-position: -200% 0; filter: blur(.2px); }
}
@keyframes qa-pulse {
  0%, 100% { opacity: 1; filter: blur(0); transform: scale(1); }
  50%      { opacity: 0.35; filter: blur(.7px); transform: scale(.94); }
}
.qa-answer p        { margin: 0 0 6px; }
.qa-answer p:last-child { margin-bottom: 0; }
.qa-answer h1,
.qa-answer h2,
.qa-answer h3       { margin: 10px 0 5px; font-weight: 600; line-height: 1.35; }
.qa-answer h1       { font-size: 15px; }
.qa-answer h2       { font-size: 14px; }
.qa-answer h3       { font-size: 13px; }
.qa-answer ul,
.qa-answer ol       { margin: 0 0 6px; padding-left: 18px; }
.qa-answer li       { margin: 2px 0; }
.qa-answer code     { font-family: var(--ol-font-mono); font-size: 12px;
                      padding: 1px 5px; border-radius: 4px;
                      background: rgba(0,0,0,0.05); }
.qa-answer pre      { margin: 0 0 6px; padding: 8px 10px;
                      border-radius: 8px; background: rgba(0,0,0,0.05);
                      overflow-x: auto; }
.qa-answer pre code { padding: 0; background: transparent; }
.qa-answer a        { color: var(--ol-blue); text-decoration: none; }
.qa-answer a:hover  { text-decoration: underline; }
.qa-answer blockquote { margin: 0 0 6px; padding: 4px 0 4px 8px;
                        border-left: 2px solid rgba(0,0,0,0.15);
                        color: var(--ol-ink-3); }
.qa-answer hr       { border: 0; border-top: 0.5px solid rgba(0,0,0,0.10);
                      margin: 8px 0; }
`;

if (typeof document !== 'undefined' && !document.getElementById('qa-panel-style')) {
  const tag = document.createElement('style');
  tag.id = 'qa-panel-style';
  tag.textContent = globalCss;
  document.head.appendChild(tag);
}
