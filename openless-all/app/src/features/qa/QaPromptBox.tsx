import { useRef, type ReactNode } from 'react';
import { ArrowUpIcon, SquareIcon } from 'lucide-react';
import './qa-prompt-box.css';

interface QaPromptBoxProps {
  value: string;
  label: string;
  placeholder: string;
  actions: ReactNode;
  feedback?: ReactNode;
  disabled?: boolean;
  readOnly?: boolean;
  sendDisabled: boolean;
  sendLabel: string;
  stop?: { label: string; disabled: boolean; onClick: () => void };
  onChange: (value: string) => void;
  onSubmit: () => void;
  onRequestKeyboard: () => void;
}

export function QaPromptBox({
  value,
  label,
  placeholder,
  actions,
  feedback,
  disabled,
  readOnly,
  sendDisabled,
  sendLabel,
  stop,
  onChange,
  onSubmit,
  onRequestKeyboard,
}: QaPromptBoxProps) {
  const composing = useRef(false);
  const canSend = !disabled && !readOnly && !sendDisabled && !stop && !!value.trim();
  return (
    <form
      className="qa-prompt"
      onSubmit={(event) => {
        event.preventDefault();
        if (canSend && !composing.current) onSubmit();
      }}
    >
      <div className="qa-input-stage" data-value={value}>
        <textarea
          value={value}
          aria-label={label}
          placeholder={placeholder}
          rows={1}
          disabled={disabled}
          readOnly={readOnly}
          onChange={(event) => onChange(event.currentTarget.value)}
          onCompositionStart={() => {
            composing.current = true;
          }}
          onCompositionEnd={() => {
            composing.current = false;
          }}
          onBlur={() => {
            composing.current = false;
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
            if (canSend) onSubmit();
          }}
          onFocus={onRequestKeyboard}
          onPointerDown={onRequestKeyboard}
        />
      </div>
      <div className="qa-prompt-tools">
        <div className="qa-prompt-actions">{actions}</div>
        {feedback}
        {stop ? (
          <button
            type="button"
            className="qa-send is-stop"
            disabled={stop.disabled}
            aria-label={stop.label}
            title={stop.label}
            onClick={stop.onClick}
          >
            <SquareIcon />
          </button>
        ) : (
          <button
            type="submit"
            className="qa-send"
            disabled={!canSend}
            aria-label={sendLabel}
            title={sendLabel}
          >
            <ArrowUpIcon />
          </button>
        )}
      </div>
    </form>
  );
}
