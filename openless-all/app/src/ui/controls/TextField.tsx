import {
  forwardRef,
  useCallback,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ComponentPropsWithoutRef,
  type ReactNode,
} from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { PasswordEye } from './PasswordEye';
import '../styles/text-field.css';

type FieldAdornmentProps = Pick<ComponentPropsWithoutRef<'div'>, 'className' | 'style'> & {
  children: ReactNode;
  searchIcon?: boolean;
};

export function FieldFrame({ className, style, children, searchIcon }: FieldAdornmentProps) {
  return (
    <div
      className={className ? `ol-field ${className}` : 'ol-field'}
      style={style}
      onMouseDown={(event) => {
        if (event.button !== 0 || event.target !== event.currentTarget) return;
        const input = event.currentTarget.querySelector<HTMLInputElement>('.ol-field-control');
        if (input && !input.disabled && input.tabIndex >= 0) {
          event.preventDefault();
          input.focus();
        }
      }}
    >
      {searchIcon && (
        <span className="ol-field-search-icon" aria-hidden="true">
          <Icon name="search" size={15} />
        </span>
      )}
      {children}
    </div>
  );
}

type TextFieldProps = Omit<ComponentPropsWithoutRef<'input'>, 'type'> &
  (
    | {
        type?: 'text' | 'url' | 'email' | 'number' | 'tel';
        visibilityLabel?: never;
        searchAction?: never;
      }
    | { type: 'search'; visibilityLabel?: never; searchAction?: ReactNode }
    | { type: 'password'; visibilityLabel?: (visible: boolean) => string; searchAction?: never }
  );

export const TextField = forwardRef<HTMLInputElement, TextFieldProps>(function TextField(
  { type = 'text', className, style, visibilityLabel, searchAction, ...props },
  ref,
) {
  return (
    <FieldFrame className={className} style={style} searchIcon={type === 'search'}>
      {type === 'password' ? (
        <SecretControl {...props} ref={ref} visibilityLabel={visibilityLabel} />
      ) : (
        <input {...props} ref={ref} type={type} className="ol-field-control" />
      )}
      {searchAction}
    </FieldFrame>
  );
});

type SecretControlProps = ComponentPropsWithoutRef<'input'> & {
  visibilityLabel?: (visible: boolean) => string;
};

const SecretControl = forwardRef<HTMLInputElement, SecretControlProps>(function SecretControl(
  { id, visibilityLabel, disabled, value, ...props },
  ref,
) {
  const { t } = useTranslation();
  const fallbackId = useId();
  const input = useRef<HTMLInputElement | null>(null);
  const [revealed, setRevealed] = useState(false);
  const visible = revealed && !disabled;
  const selection = useRef<{
    start: number;
    end: number;
    direction: 'forward' | 'backward' | 'none' | null;
    scrollLeft: number;
  } | null>(null);
  const setInput = useCallback(
    (element: HTMLInputElement | null) => {
      input.current = element;
      if (typeof ref === 'function') ref(element);
      else if (ref) ref.current = element;
    },
    [ref],
  );

  useLayoutEffect(() => {
    const el = input.current;
    const saved = selection.current;
    selection.current = null;
    if (saved && el && el.ownerDocument.activeElement === el) {
      // Switching the native input type can reset the selection and scroll position.
      el.setSelectionRange(saved.start, saved.end, saved.direction ?? undefined);
      el.scrollLeft = saved.scrollLeft;
    }
  }, [visible]);

  useLayoutEffect(() => {
    if (disabled || value === '') setRevealed(false);
  }, [disabled, value]);

  const label = visibilityLabel?.(visible) ?? t(visible ? 'common.hide' : 'common.show');
  return (
    <>
      <input
        autoComplete="off"
        autoCapitalize="none"
        autoCorrect="off"
        spellCheck={false}
        {...props}
        id={id ?? fallbackId}
        ref={setInput}
        type={visible ? 'text' : 'password'}
        className="ol-field-control ol-field-secret"
        value={value}
        disabled={disabled}
      />
      <button
        className="ol-field-visibility"
        type="button"
        aria-label={label}
        aria-controls={id ?? fallbackId}
        title={label}
        disabled={disabled}
        onMouseDown={(event) => {
          if (input.current === event.currentTarget.ownerDocument.activeElement) {
            event.preventDefault();
          }
        }}
        onClick={() => {
          const el = input.current;
          if (
            el &&
            el.ownerDocument.activeElement === el &&
            el.selectionStart !== null &&
            el.selectionEnd !== null
          ) {
            selection.current = {
              start: el.selectionStart,
              end: el.selectionEnd,
              direction: el.selectionDirection,
              scrollLeft: el.scrollLeft,
            };
          }
          setRevealed((shown) => !shown);
        }}
      >
        <PasswordEye open={visible} />
      </button>
    </>
  );
});
