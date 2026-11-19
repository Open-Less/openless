import { TextField } from '../../ui/controls/TextField';
import { useId } from 'react';
import { useTranslation } from 'react-i18next';

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
  return (
    <div className="ol-sync-form-field">
      <label htmlFor={id}>{label}</label>
      <TextField
        id={id}
        type="password"
        value={value}
        onChange={(event) => onChange(event.currentTarget.value)}
        autoComplete={autoComplete}
        aria-invalid={errorId ? true : undefined}
        aria-describedby={errorId}
        maxLength={4096}
        disabled={disabled}
        visibilityLabel={(visible) =>
          t(visible ? 'cloudSyncE2ee.concealPassword' : 'cloudSyncE2ee.revealPassword', {
            field: label,
          })
        }
      />
    </div>
  );
}
