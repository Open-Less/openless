import type { CSSProperties, ReactNode } from 'react';
import { Icon } from '../../components/Icon';
import { useLayoutStack, useConservativeLayout } from '../../lib/useMobileLayout';

export function ChannelFormRow({
  label,
  htmlFor,
  children,
}: {
  label: string;
  htmlFor?: string;
  children: ReactNode;
}) {
  const stack = useLayoutStack();
  const conservative = useConservativeLayout();
  return (
    <div className="ol-channel-form-row" data-stacked={stack || conservative ? 'true' : undefined}>
      <label htmlFor={htmlFor}>{label}</label>
      <div style={{ minWidth: 0, width: '100%' }}>{children}</div>
    </div>
  );
}

export function ChannelSectionHeading({
  title,
  description,
  icon,
}: {
  title: string;
  description?: string;
  icon?: string;
}) {
  return (
    <div className="ol-channel-section-heading" style={{ marginBottom: 6 }}>
      <h3
        style={{
          margin: 0,
          fontSize: 14,
          color: 'var(--ol-ink)',
          fontWeight: 'var(--ol-font-weight-semibold)',
        }}
      >
        {icon && <Icon name={icon} size={15} />}
        {title}
      </h3>
      {description && (
        <p style={{ margin: '6px 0 0', color: 'var(--ol-ink-3)', fontSize: 12, lineHeight: 1.65 }}>
          {description}
        </p>
      )}
    </div>
  );
}

/** Sentinel value for the "Custom model…" entry in the model preset dropdown; selecting it switches back to a manual input. */
export const CUSTOM_MODEL_OPTION_VALUE = '__custom_model__';

export function matchesEndpointPreset(value: string, endpoint: string): boolean {
  try {
    const current = new URL(value.trim());
    const preset = new URL(endpoint);
    const basePath = (path: string) =>
      path
        .replace(/\/$/, '')
        .replace(/\/(chat\/completions|responses|messages|models)$/, '')
        .replace(/\/v3$/, '');
    return (
      current.origin === preset.origin &&
      !current.search &&
      !current.hash &&
      !current.username &&
      !current.password &&
      basePath(current.pathname) === basePath(preset.pathname)
    );
  } catch {
    return false;
  }
}

export const iconBtnStyle: CSSProperties = {
  width: 32,
  height: 32,
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 'var(--ol-control-radius)',
  background: 'var(--ol-control-solid)',
  boxShadow: '0 1px 2px rgba(0,0,0,0.04)',
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  color: 'var(--ol-ink-3)',
  cursor: 'pointer',
  flexShrink: 0,
  transition:
    'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick), transform 0.12s var(--ol-motion-quick)',
};
