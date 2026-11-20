import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { PageSkeleton } from '../../../components/ui/PageSkeleton';
import { Button } from '../../../ui/controls/Button';
import { SettingRow, Toggle } from '../../../ui/controls/SettingRow';
import { Card } from '../../../ui/layout/Card';
import { useInteraction } from '../../../ui/overlays/InteractionProvider';
import {
  readAskEntrySettings,
  writeAskEntrySettings,
  type AskEntrySettings,
  type AskEntryIntentMode,
} from './askEntrySettings';

const modes = ['semanticHybrid', 'separateModel', 'mainModelTool'] as const satisfies readonly AskEntryIntentMode[];

const recoveryNoticeKey = 'ask-entry-settings-recovery';

function useAskEntryPreference<K extends keyof AskEntrySettings>(key: K) {
  const { t } = useTranslation();
  const { notify } = useInteraction();
  const [settings, setSettings] = useState<AskEntrySettings[K] | 'readFailed'>();
  const [saving, setSaving] = useState(false);
  const writePending = useRef(false);

  useEffect(() => {
    let active = true;
    void readAskEntrySettings().then(
      (committed) => {
        if (active) setSettings(committed[key]);
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
  }, [key, t, notify]);

  function commit(next: Partial<AskEntrySettings>) {
    if (writePending.current) return;
    const restoring = settings === 'readFailed';
    writePending.current = true;
    setSaving(true);
    void writeAskEntrySettings(next)
      .then(
        (committed) => {
          setSettings(committed[key]);
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

  return { settings, saving, commit };
}

export function AskEntrySettingsSection() {
  const { t } = useTranslation();
  const { settings, saving, commit } = useAskEntryPreference('autoVoiceOnOpen');
  if (settings === undefined) return <PageSkeleton rows={1} />;
  if (settings === 'readFailed') {
    return <AskSettingsRecovery saving={saving} commit={commit} />;
  }
  return (
    <Card>
      <SettingRow
        label={t('askEntrySettings.autoVoiceOnOpen')}
        desc={t('askEntrySettings.autoVoiceHint')}
      >
        <Toggle
          on={settings}
          disabled={saving}
          onToggle={(autoVoiceOnOpen) => commit({ autoVoiceOnOpen })}
        />
      </SettingRow>
    </Card>
  );
}

export function IntentRoutingSection() {
  const { t } = useTranslation();
  const { settings, saving, commit } = useAskEntryPreference('intentMode');
  if (settings === undefined) return <PageSkeleton rows={3} />;
  if (settings === 'readFailed') {
    return <AskSettingsRecovery saving={saving} commit={commit} />;
  }
  return (
    <Card>
      <fieldset className="ol-intent-routing-options" disabled={saving}>
        <legend>{t('askEntrySettings.routingHint')}</legend>
        {modes.map((intentMode) => {
          const label = t(`askEntrySettings.${intentMode}`);
          return (
            <SettingRow key={intentMode} label={label} desc={t(`askEntrySettings.${intentMode}Hint`)}>
              <input
                type="radio"
                aria-label={label}
                name="ask-intent-routing"
                checked={settings === intentMode}
                onChange={() => commit({ intentMode })}
              />
            </SettingRow>
          );
        })}
      </fieldset>
    </Card>
  );
}

function AskSettingsRecovery({
  saving,
  commit,
}: {
  saving: boolean;
  commit: (settings: AskEntrySettings) => void;
}) {
  const { t } = useTranslation();
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
