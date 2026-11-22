import { useEffect, useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';

export function SyncPasswordField({
  label,
  value,
  onChange,
  autoComplete,
  errorId,
  disabled,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  autoComplete: string;
  errorId?: string;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const id = useId();
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    if (!value) setVisible(false);
  }, [value]);
  const visibilityLabel = t(
    visible ? 'cloudSyncE2ee.concealPassword' : 'cloudSyncE2ee.revealPassword',
    { field: label },
  );
  return (
    <div className="ol-sync-password-field">
      <label htmlFor={id}>{label}</label>
      <div className="ol-sync-password-input">
        <input
          id={id}
          type={visible ? 'text' : 'password'}
          value={value}
          onChange={(event) => onChange(event.currentTarget.value)}
          autoComplete={autoComplete}
          aria-invalid={errorId ? true : undefined}
          aria-describedby={errorId}
          maxLength={4096}
          disabled={disabled}
        />
        <button
          type="button"
          className="ol-sync-password-visibility"
          disabled={disabled}
          aria-label={visibilityLabel}
          aria-controls={id}
          title={visibilityLabel}
          onMouseDown={(event) => {
            if (document.activeElement?.id === id) event.preventDefault();
          }}
          onClick={() => setVisible((shown) => !shown)}
        >
          <Icon name={visible ? 'eyeOff' : 'eye'} size={16} />
        </button>
      </div>
    </div>
  );
}
