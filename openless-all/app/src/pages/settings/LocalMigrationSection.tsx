import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getPlatformCapabilities, type PlatformCapabilities } from '../../lib/platform';
import { openAndroidLocalMigration } from '../../lib/ipc';
import { Btn, Card } from '../_atoms';
import { SectionTitle } from './shared';

/** Application-level Android entry point for the native local migration flow. */
export function LocalMigrationSection() {
  const { t } = useTranslation();
  const [platform, setPlatform] = useState<PlatformCapabilities | null>(null);
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState(false);

  useEffect(() => {
    let alive = true;
    void getPlatformCapabilities().then((next) => {
      if (alive) setPlatform(next);
    });
    return () => {
      alive = false;
    };
  }, []);

  if (platform?.platform !== 'android') return null;

  const openMigration = async () => {
    setOpening(true);
    setError(false);
    try {
      await openAndroidLocalMigration();
    } catch {
      setError(true);
    } finally {
      setOpening(false);
    }
  };

  return (
    <Card>
      <SectionTitle>{t('settings.localMigration.title')}</SectionTitle>
      <p
        style={{
          margin: '0 0 12px',
          color: 'var(--ol-ink-3)',
          fontSize: 12,
          lineHeight: 1.6,
        }}
      >
        {t('settings.localMigration.description')}
      </p>
      <p
        style={{
          margin: '0 0 14px',
          color: 'var(--ol-ink-4)',
          fontSize: 11,
          lineHeight: 1.55,
        }}
      >
        {t('settings.localMigration.safetyNote')}
      </p>
      <Btn variant="primary" size="sm" onClick={() => void openMigration()} disabled={opening}>
        {opening ? t('settings.localMigration.opening') : t('settings.localMigration.open')}
      </Btn>
      {error && (
        <p role="alert" style={{ margin: '10px 0 0', color: 'var(--ol-danger)', fontSize: 12 }}>
          {t('settings.localMigration.openFailed')}
        </p>
      )}
    </Card>
  );
}
