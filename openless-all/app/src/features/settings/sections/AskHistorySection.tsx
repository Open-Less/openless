import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { PageSkeleton } from '../../../components/ui/PageSkeleton';
import type { AskReadPermissions } from '../../../api/ask-turn-reference';
import { useHotkeySettings } from '../../../data/preferences';
import { isDesktop } from '../../../lib/platform';
import { invokeOrPreview } from '../../../runtime/transport';
import { getRuntimeOS, isPreviewHost } from '../../../runtime/environment';
import { SettingRow, Toggle } from '../../../ui/controls/SettingRow';
import { Card } from '../../../ui/layout/Card';
import { useInteraction } from '../../../ui/overlays/InteractionProvider';

const DEFAULT_READ_PERMISSIONS: AskReadPermissions = {
  browserTab: false,
  browserSelection: false,
  voiceHistory: false,
};

function readAskPermissions() {
  return invokeOrPreview('ask_get_read_permissions', undefined, () => DEFAULT_READ_PERMISSIONS);
}

export function AskHistorySection({ compact = false }: { compact?: boolean }) {
  const { t } = useTranslation();
  const { notify } = useInteraction();
  const { prefs, updatePrefs } = useHotkeySettings();
  const supportsOriginalSelection = getRuntimeOS() === 'mac' &&
    (isPreviewHost() || /Macintosh|Mac OS X/i.test(navigator.userAgent));
  const [contextAllowed, setContextAllowed] = useState<boolean>();
  const [contextSaving, setContextSaving] = useState(false);
  const [readPermissions, setReadPermissions] = useState<AskReadPermissions>();
  const [readSaving, setReadSaving] = useState<keyof AskReadPermissions>();
  const readPending = useRef(false);
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
    void readAskPermissions()
      .then((permissions) => { if (!cancelled) setReadPermissions(permissions); })
      .catch(() => { if (!cancelled) notify({ state: 'failed', message: t('selectionAsk.desktopContext.saveFailed') }); });
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
            disabled={contextSaving || readSaving !== undefined}
            onToggle={(allowed) => {
              if (readPending.current) return;
              readPending.current = true;
              setContextSaving(true);
              void invokeOrPreview('ask_set_context_permission', { allowed }, () => undefined)
                .then(() => {
                  setContextAllowed(allowed);
                  if (!allowed) setReadPermissions((current) => current && { ...current, browserSelection: false });
                })
                .catch(async () => {
                  notify({ state: 'failed', message: t('selectionAsk.desktopContext.saveFailed') });
                  // Revocation may take effect in memory even if persistence fails.
                  const [context, permissions] = await Promise.allSettled([
                    invokeOrPreview('ask_get_context_permission', undefined, () => contextAllowed),
                    readAskPermissions(),
                  ]);
                  if (context.status === 'fulfilled') setContextAllowed(context.value);
                  if (permissions.status === 'fulfilled') setReadPermissions(permissions.value);
                })
                .finally(() => {
                  readPending.current = false;
                  setContextSaving(false);
                });
            }}
          />
        </SettingRow>
      )}
      {readPermissions && (['browserTab', 'browserSelection', 'voiceHistory'] as const)
        .filter((permission) => permission !== 'browserSelection' || supportsOriginalSelection)
        .map((permission) => (
        <SettingRow key={permission}
          label={t(permission === 'browserSelection' ? 'askTurnReference.originalInputAccessibility' : `askEntrySettings.${permission}`)}
          desc={t(permission === 'browserSelection' ? 'askTurnReference.permissionHint' : `askEntrySettings.${permission}Hint`)}
          descriptionPlacement="hint"
        >
          <Toggle
            on={Boolean(readPermissions[permission])}
            disabled={contextSaving || readSaving !== undefined ||
              (permission === 'browserSelection' && (!contextAllowed || !readPermissions.browserTab))}
            onToggle={(allowed) => {
              if (readPending.current) return;
              readPending.current = true;
              setReadSaving(permission);
              const permissions = { ...readPermissions, [permission]: allowed };
              if (!permissions.browserTab || !contextAllowed) permissions.browserSelection = false;
              void invokeOrPreview<AskReadPermissions>('ask_set_read_permissions', { permissions }, () => permissions)
                .then(setReadPermissions)
                .catch(async () => {
                  notify({ state: 'failed', message: t('selectionAsk.desktopContext.saveFailed') });
                  await readAskPermissions().then(setReadPermissions).catch(() => {});
                })
                .finally(() => { readPending.current = false; setReadSaving(undefined); });
            }}
          />
        </SettingRow>
      ))}
    </Card>
  );
}
