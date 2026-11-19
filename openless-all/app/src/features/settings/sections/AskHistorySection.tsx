import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { PageSkeleton } from '../../../components/ui/PageSkeleton';
import { useHotkeySettings } from '../../../data/preferences';
import { isDesktop } from '../../../lib/platform';
import { invokeOrPreview } from '../../../runtime/transport';
import { SettingRow, Toggle } from '../../../ui/controls/SettingRow';
import { Card } from '../../../ui/layout/Card';

export function AskHistorySection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const [saveFailed, setSaveFailed] = useState(false);
  const [contextAllowed, setContextAllowed] = useState<boolean>();
  const [contextSaving, setContextSaving] = useState(false);
  const [contextError, setContextError] = useState(false);
  useEffect(() => {
    if (!isDesktop()) return;
    let cancelled = false;
    void invokeOrPreview('ask_get_context_permission', undefined, () => false)
      .then((allowed) => { if (!cancelled) setContextAllowed(allowed); })
      .catch(() => { if (!cancelled) setContextError(true); });
    return () => { cancelled = true; };
  }, []);
  if (!prefs) return <PageSkeleton rows={1} />;

  return (
    <Card>
      <SettingRow
        label={t('selectionAsk.history.title')}
        desc={t('selectionAsk.history.desc')}
        descriptionPlacement="inline"
      >
        <Toggle
          on={prefs.qaSaveHistory}
          onToggle={(qaSaveHistory) => {
            setSaveFailed(false);
            void updatePrefs((current) => ({ ...current, qaSaveHistory })).catch(() =>
              setSaveFailed(true),
            );
          }}
        />
      </SettingRow>
      {saveFailed && <p role="alert">{t('selectionAsk.save.historySaveFailed')}</p>}
      {contextAllowed !== undefined && (
        <SettingRow
          label={t('selectionAsk.desktopContext.title', 'Share desktop app names with Ask')}
          desc={t(
            'selectionAsk.desktopContext.description',
            'When you open Ask, include the focused app and the names of open apps in the answer request. Window contents are not included.',
          )}
          descriptionPlacement="inline"
        >
          <Toggle
            on={contextAllowed}
            disabled={contextSaving}
            onToggle={(allowed) => {
              setContextSaving(true);
              setContextError(false);
              void invokeOrPreview('ask_set_context_permission', { allowed }, () => undefined)
                .then(() => setContextAllowed(allowed))
                .catch(() => setContextError(true))
                .finally(() => setContextSaving(false));
            }}
          />
        </SettingRow>
      )}
      {contextError && (
        <p role="alert">{t('selectionAsk.desktopContext.saveFailed', 'Could not update Ask’s desktop context permission.')}</p>
      )}
    </Card>
  );
}
