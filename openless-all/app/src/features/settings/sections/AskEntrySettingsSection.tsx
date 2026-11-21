import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { PageSkeleton } from '../../../components/ui/PageSkeleton';
import { Button } from '../../../ui/controls/Button';
import { SettingRow, Toggle } from '../../../ui/controls/SettingRow';
import { Card } from '../../../ui/layout/Card';
import { SectionTitle } from '../../../ui/layout/SectionTitle';
import { useInteraction } from '../../../ui/overlays/InteractionProvider';
import {
  readAskEntrySettings,
  writeAskEntrySettings,
  type AskEntrySettings,
  type AskEntryIntentMode,
} from './askEntrySettings';

const betaModes = [
  'separateModel',
  'mainModelTool',
] as const satisfies readonly AskEntryIntentMode[];

const recoveryNoticeKey = 'ask-entry-settings-recovery';

export function AskEntrySettingsSection() {
  const { t } = useTranslation();
  const { notify } = useInteraction();
  const [settings, setSettings] = useState<AskEntrySettings | 'readFailed'>();
  const [saving, setSaving] = useState(false);
  const writePending = useRef(false);

  useEffect(() => {
    let active = true;
    void readAskEntrySettings().then(
      (committed) => {
        if (active) setSettings(committed);
      },
      () => {
        if (!active) return;
        setSettings((current) => current ?? 'readFailed');
        notify({
          key: recoveryNoticeKey,
          state: 'failed',
          message: t('askEntrySettings.loadFailed'),
        });
      },
    );
    return () => {
      active = false;
    };
  }, [t, notify]);

  function commit(next: AskEntrySettings) {
    if (writePending.current) return;
    const restoring = settings === 'readFailed';
    writePending.current = true;
    setSaving(true);
    void writeAskEntrySettings(next)
      .then(
        (committed) => {
          setSettings(committed);
          if (restoring) {
            notify({ key: recoveryNoticeKey, state: 'saved', message: t('common.saved') });
          }
        },
        () =>
          notify({
            key: restoring ? recoveryNoticeKey : undefined,
            state: 'failed',
            message: t('askEntrySettings.saveFailed'),
          }),
      )
      .finally(() => {
        writePending.current = false;
        setSaving(false);
      });
  }

  if (!settings) return <PageSkeleton rows={3} />;
  if (settings === 'readFailed') {
    return (
      <Card>
        <SettingRow
          label={t('askEntrySettings.loadFailed')}
          desc={t('askEntrySettings.restoreDefaultsHint')}
        >
          <Button
            size="sm"
            disabled={saving}
            onClick={() => commit({ autoVoiceOnOpen: true, intentMode: 'separateModel' })}
          >
            {t('askEntrySettings.restoreDefaults')}
          </Button>
        </SettingRow>
      </Card>
    );
  }
  return (
    <>
      <Card>
        <SettingRow
          label={t('askEntrySettings.autoVoiceOnOpen')}
          desc={t('askEntrySettings.autoVoiceHint')}
        >
          <Toggle
            on={settings.autoVoiceOnOpen}
            disabled={saving}
            onToggle={(autoVoiceOnOpen) => commit({ ...settings, autoVoiceOnOpen })}
          />
        </SettingRow>
      </Card>
      <Card>
        <SectionTitle hint={t('askEntrySettings.routingHint')}>
          {t('askEntrySettings.routing')}
        </SectionTitle>
        <SettingRow label={t('askEntrySettings.currentMode')}>
          <span aria-live="polite">{t(`askEntrySettings.${settings.intentMode}`)}</span>
        </SettingRow>
        {betaModes.map((mode) => {
          const label = t(`askEntrySettings.${mode}`);
          const enabled = settings.intentMode === mode;
          return (
            <SettingRow key={mode} label={label} desc={t(`askEntrySettings.${mode}Hint`)}>
              <Toggle
                label={`${label} (Beta)`}
                on={enabled}
                disabled={saving}
                onToggle={(next) => {
                  if (next !== enabled) {
                    commit({ ...settings, intentMode: next ? mode : 'semanticHybrid' });
                  }
                }}
              />
              <span className="ol-beta-tag" aria-hidden="true">
                Beta
              </span>
            </SettingRow>
          );
        })}
      </Card>
    </>
  );
}
