import { useContext, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { readCredential, setCredential } from '../../lib/ipc';
import {
  bailianProtocols,
  readBailianProtocol,
  writeBailianProtocol,
  type BailianProtocol,
} from '../../lib/bailianProtocol';
import { ProviderFormContext } from './ProviderForm';
import { inputStyle } from './shared';
import { emitSaved } from '../../lib/savedEvent';

const account = 'asr.advanced_config';
export function BailianProtocolField({
  channelId,
  onChange,
  onUserMutation,
  onBlockedChange,
}: {
  channelId: string;
  onChange: (protocol: BailianProtocol) => void;
  onUserMutation?: () => void;
  onBlockedChange: (account: string, blocked: boolean) => void;
}) {
  const { t } = useTranslation();
  const form = useContext(ProviderFormContext);
  const track = form?.track;
  const register = form?.register;
  const [selected, setSelected] = useState<BailianProtocol>('auto');
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const pending = useRef<Promise<boolean>>(Promise.resolve(true));
  const writing = useRef(false);
  const mounted = useRef(false);
  const canLeave = useRef(false);
  canLeave.current = loaded && !error;

  useEffect(() => {
    mounted.current = true;
    let cancelled = false;
    readCredential(account, channelId)
      .then((raw) => {
        if (cancelled) return;
        const protocol = readBailianProtocol(raw);
        setSelected(protocol);
        onChange(protocol);
        setLoaded(true);
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
      });
    return () => {
      cancelled = true;
      mounted.current = false;
    };
  }, [channelId, onChange]);

  useEffect(
    () => register?.(account, async () => (await pending.current) && canLeave.current),
    [register],
  );
  useEffect(() => {
    const blocked = !loaded || saving || Boolean(error);
    track?.(account, blocked);
    onBlockedChange(account, blocked);
    return () => {
      track?.(account, false);
      onBlockedChange(account, false);
    };
  }, [loaded, saving, error, track, onBlockedChange]);

  const save = (protocol: BailianProtocol) => {
    if (writing.current || !loaded) return;
    writing.current = true;
    setSaving(true);
    setError('');
    form?.invalidate(account);
    track?.(account, true);
    onBlockedChange(account, true);
    onUserMutation?.();
    pending.current = (async () => {
      try {
        const raw = await readCredential(account, channelId);
        await setCredential(account, writeBailianProtocol(raw, protocol), channelId);
        if (mounted.current) {
          setSelected(protocol);
          onChange(protocol);
          emitSaved('saved', t('common.saved'));
        }
        return true;
      } catch (err) {
        if (mounted.current) setError(String(err));
        return false;
      } finally {
        writing.current = false;
        if (mounted.current) setSaving(false);
      }
    })();
  };

  return (
    <label style={{ display: 'grid', gap: 6, marginTop: 8 }}>
      <span>{t('settings.providers.bailianProtocolLabel')}</span>
      <select
        style={inputStyle}
        value={selected}
        disabled={!loaded || saving || form?.leaving}
        onChange={(event) => save(event.target.value as BailianProtocol)}
      >
        {bailianProtocols.map((protocol) => (
          <option key={protocol} value={protocol}>
            {t(`settings.providers.bailianProtocolOptions.${protocol}`)}
          </option>
        ))}
      </select>
      <span style={{ fontSize: 11.5, color: 'var(--ol-ink-4)' }}>
        {t('settings.providers.bailianProtocolNote')}
      </span>
      {error && <span role="alert">{error}</span>}
    </label>
  );
}
