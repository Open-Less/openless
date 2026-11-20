import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { PageSkeleton } from '../../../components/ui/PageSkeleton';
import { useHotkeySettings } from '../../../data/preferences';
import { isDesktop } from '../../../lib/platform';
import { invokeOrPreview } from '../../../runtime/transport';
import { SettingRow, Toggle } from '../../../ui/controls/SettingRow';
import { Card } from '../../../ui/layout/Card';
import { useInteraction } from '../../../ui/overlays/InteractionProvider';

export function AskHistorySection({ compact = false }: { compact?: boolean }) {
  const { t } = useTranslation();
  const { notify } = useInteraction();
  const { prefs, updatePrefs } = useHotkeySettings();
  const [contextAllowed, setContextAllowed] = useState<boolean>();
  const [contextSaving, setContextSaving] = useState(false);
  useEffect(() => {
    if (!isDesktop()) return;
    let cancelled = false;
    void invokeOrPreview('ask_get_context_permission', undefined, () => false)
      .then((allowed) => {
        if (!cancelled) setContextAllowed(allowed);
      })
      .catch(() => {
        if (!cancelled) {
          notify({ state: 'failed', message: t('selectionAsk.desktopContext.saveFailed') });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [t, notify]);
  if (!prefs) return <PageSkeleton rows={1} />;

  return (
    <Card>
      <SettingRow
        label={t('selectionAsk.history.title')}
        desc={t('selectionAsk.history.desc')}
        descriptionPlacement={compact ? 'hint' : 'inline'}
      >
        <Toggle
          on={prefs.qaSaveHistory}
          onToggle={(qaSaveHistory) => {
            void updatePrefs((current) => ({ ...current, qaSaveHistory })).catch(() =>
              notify({ state: 'failed', message: t('selectionAsk.save.historySaveFailed') }),
            );
          }}
        />
      </SettingRow>
      {contextAllowed !== undefined && (
        <SettingRow
          label={t('selectionAsk.desktopContext.title', 'Share desktop app names with Ask')}
          desc={t(
            'selectionAsk.desktopContext.description',
            'When you open Ask, include the focused app and the names of open apps in the answer request. Window contents are not included.',
          )}
          descriptionPlacement={compact ? 'hint' : 'inline'}
        >
          <Toggle
            on={contextAllowed}
            disabled={contextSaving}
            onToggle={(allowed) => {
              setContextSaving(true);
              void invokeOrPreview('ask_set_context_permission', { allowed }, () => undefined)
                .then(() => setContextAllowed(allowed))
                .catch(() =>
                  notify({ state: 'failed', message: t('selectionAsk.desktopContext.saveFailed') }),
                )
                .finally(() => setContextSaving(false));
            }}
          />
        </SettingRow>
      )}
    </Card>
  );
}
