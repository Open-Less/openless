import { AnimatePresence, motion } from 'framer-motion';
import { ArrowUpIcon, SquareIcon } from 'lucide-react';
import { useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from 'react';
import { useReducedMotion } from '../../lib/motion';
import './qa-prompt-box.css';

interface QaPromptBoxProps {
  value: string;
  label: string;
  placeholder: string;
  actions?: ReactNode;
  feedback?: ReactNode;
  disabled?: boolean;
  readOnly?: boolean;
  recording?: boolean;
  sendDisabled: boolean;
  sendLabel: string;
  stop?: { label: string; disabled: boolean; onClick: () => void };
  onChange: (value: string) => void;
  onSubmit: () => void;
  onRequestKeyboard: () => void;
  editorRef?: RefObject<HTMLTextAreaElement>;
  onEditorFocusChange?: (focused: boolean) => void;
}

interface SentDraft {
  text: string;
  height: number;
  scroll: number;
}

const transition = { type: 'spring', duration: 0.4, bounce: 0.1 } as const;

export function QaPromptBox({
  value,
  label,
  placeholder,
  actions,
  feedback,
  disabled,
  readOnly,
  recording,
  sendDisabled,
  sendLabel,
  stop,
  onChange,
  onSubmit,
  onRequestKeyboard,
  editorRef,
  onEditorFocusChange,
}: QaPromptBoxProps) {
  const composing = useRef(false);
  const fallbackEditor = useRef<HTMLTextAreaElement>(null);
  const input = editorRef ?? fallbackEditor;
  const measure = useRef<HTMLDivElement>(null);
  const submitted = useRef<SentDraft | null>(null);
  const [flight, setFlight] = useState<SentDraft | null>(null);
  const [height, setHeight] = useState(24);
  const reduced = useReducedMotion();
  const canSend = !disabled && !readOnly && !sendDisabled && !stop && !!value.trim();
  const submit = () => {
    if (!canSend || composing.current) return;
    if (!reduced && input.current) {
      submitted.current = {
        text: value,
        height: input.current.clientHeight,
        scroll: input.current.scrollTop,
      };
    }
    onSubmit();
  };

  useLayoutEffect(() => {
    const field = measure.current;
    if (!field) return;
    const resize = () => setHeight(field.offsetHeight);
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(field);
    return () => observer.disconnect();
  }, []);

  useLayoutEffect(() => {
    if (!value && submitted.current) {
      setFlight(submitted.current);
      submitted.current = null;
    } else if (value) {
      setFlight(null);
    }
  }, [value]);

  const tokens = flight?.text.split(/(\s+)/) ?? [];
  return (
    <form
      className="qa-prompt"
      data-recording={recording || undefined}
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      <div className="qa-input-stage">
        <div ref={measure} className="qa-input-measure" aria-hidden="true">
          {value}{' '}
        </div>
        <motion.div
          className="qa-input-field"
          initial={false}
          animate={{ height }}
          transition={reduced ? { duration: 0 } : transition}
        >
          <AnimatePresence initial={false}>
            {!value && !flight && (
              <motion.span
                className="qa-input-placeholder"
                key={placeholder}
                aria-hidden="true"
                initial={reduced ? false : { opacity: 0, y: 6, filter: 'blur(4px)' }}
                animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
                exit={reduced ? { opacity: 0 } : { opacity: 0, y: -6, filter: 'blur(4px)' }}
                transition={reduced ? { duration: 0 } : transition}
              >
                {placeholder}
              </motion.span>
            )}
          </AnimatePresence>
          <textarea
            ref={input}
            value={value}
            aria-label={label}
            placeholder={placeholder}
            rows={1}
            disabled={disabled}
            readOnly={readOnly}
            onChange={(event) => {
              submitted.current = null;
              onChange(event.currentTarget.value);
            }}
            onCompositionStart={() => {
              composing.current = true;
            }}
            onCompositionEnd={() => {
              composing.current = false;
            }}
            onBlur={() => {
              composing.current = false;
              onEditorFocusChange?.(false);
            }}
            onKeyDown={(event) => {
              if (event.key === ' ' && event.getModifierState('Fn')) event.preventDefault();
              if (
                event.key !== 'Enter' ||
                composing.current ||
                event.nativeEvent.isComposing ||
                event.keyCode === 229 ||
                event.shiftKey
              )
                return;
              event.preventDefault();
              if (!event.repeat) submit();
            }}
            onFocus={() => {
              onRequestKeyboard();
              onEditorFocusChange?.(true);
            }}
            onPointerDown={onRequestKeyboard}
          />
        </motion.div>
        {flight && (
          <motion.div
            aria-hidden="true"
            className="qa-input-flight"
            style={{ height: flight.height + 24 }}
            initial={{ opacity: 1 }}
            animate={{ opacity: 0 }}
            transition={{ duration: 0.38, delay: 0.1 }}
            onAnimationComplete={() => setFlight(null)}
          >
            <div style={{ marginTop: -flight.scroll }}>
              {tokens.length <= 100 ? (
                tokens.map((token, index) =>
                  token.trim() ? (
                    <motion.span
                      key={index}
                      animate={{ y: -24, opacity: 0, filter: 'blur(3px)' }}
                      transition={{ duration: 0.35, delay: Math.min(0.1, index * 0.014) }}
                    >
                      {token}
                    </motion.span>
                  ) : (
                    token
                  ),
                )
              ) : (
                <motion.div animate={{ y: -12 }} transition={{ duration: 0.38 }}>
                  {flight.text}
                </motion.div>
              )}
            </div>
          </motion.div>
        )}
      </div>
      <div className="qa-prompt-tools">
        <div className="qa-prompt-actions">{actions}</div>
        {feedback}
        <button
          type={stop ? 'button' : 'submit'}
          className={`qa-send${stop ? ' is-stop' : ''}`}
          disabled={stop ? stop.disabled : !canSend}
          aria-label={stop?.label ?? sendLabel}
          title={stop?.label ?? sendLabel}
          onMouseDown={(event) => {
            // Clicking send keeps the active editor focused for the next prompt.
            if (event.button === 0 && document.activeElement === input.current)
              event.preventDefault();
          }}
          onClick={stop?.onClick}
        >
          <AnimatePresence initial={false} mode="popLayout">
            <motion.span
              key={stop ? 'stop' : 'send'}
              initial={reduced ? false : { opacity: 0, scale: 0.45 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: reduced ? 1 : 0.45 }}
              transition={reduced ? { duration: 0 } : transition}
              aria-hidden="true"
            >
              {stop ? <SquareIcon /> : <ArrowUpIcon />}
            </motion.span>
          </AnimatePresence>
        </button>
      </div>
    </form>
  );
}
