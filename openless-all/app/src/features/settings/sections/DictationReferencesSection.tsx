import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  openDictationReferencesPermissionSettings,
  type DictationReferencesStatus,
} from '../../../api/dictationReferences';
import { KbdGroup } from '../../../components/Kbd';
import { useHotkeySettings } from '../../../data/preferences';
import { formatComboParts } from '../../../lib/hotkey';
import { Button } from '../../../ui/controls/Button';
import { SelectLite } from '../../../ui/controls/SelectLite';
import { SettingRow, Toggle } from '../../../ui/controls/SettingRow';
import { Card } from '../../../ui/layout/Card';
import { ExperimentalSectionTitle } from '../../../ui/layout/ExperimentalSectionTitle';
import { dictationReferenceKeyOptions, saveDictationReferencePreference } from './dictationReferenceKey';
import './dictation-references.css';

export function DictationReferencesSection({
  screenCapture,
  onPermissionSettingsUnavailable,
}: {
  screenCapture?: DictationReferencesStatus['screenCapture'];
  onPermissionSettingsUnavailable: () => void;
}) {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const [saving, setSaving] = useState(false);
  const [keyConflict, setKeyConflict] = useState(false);
  if (!prefs) return null;

  const enabled = prefs.dictationReferencesEnabled ?? false;
  const recordingKeys = prefs.dictationHotkey?.primary.trim() ? formatComboParts(prefs.dictationHotkey) : [];
  const save = async (edits: { dictationReferencesEnabled: boolean } | { dictationReferenceKey: string }) => {
    setKeyConflict(false);
    setSaving(true);
    const saved = await saveDictationReferencePreference(() =>
      updatePrefs((current) => ({ ...current, ...edits }), { reportFailure: false }),
    );
    setKeyConflict(!saved);
    setSaving(false);
  };

  return (
    <Card className="ol-dictation-references">
      <ExperimentalSectionTitle badge={t('common.experimental')}>
        {t('settings.advancedPages.dictationReferencesTitle')}
      </ExperimentalSectionTitle>
      <SettingRow
        label={t('settings.dictationReferences.enabledLabel')}
        desc={t('settings.dictationReferences.enabledDesc')}
        descriptionPlacement="inline"
      >
        <Toggle
          on={enabled}
          disabled={saving}
          onToggle={(next) => void save({ dictationReferencesEnabled: next })}
        />
      </SettingRow>
      <div className="ol-dictation-reference-key" aria-disabled={!enabled || undefined}>
        <SettingRow
          label={t('settings.dictationReferences.keyLabel')}
          desc={t('settings.dictationReferences.keyDesc')}
          descriptionPlacement="inline"
        >
          {recordingKeys.length > 0 && (
            <>
              <KbdGroup keys={recordingKeys} style={{ flexWrap: 'wrap', maxWidth: '100%' }} />
              <span aria-hidden="true">+</span>
            </>
          )}
          <SelectLite
            value={prefs.dictationReferenceKey ?? 'S'}
            onChange={(key) => void save({ dictationReferenceKey: key })}
            options={dictationReferenceKeyOptions}
            ariaLabel={t('settings.dictationReferences.keyLabel')}
            disabled={!enabled || saving}
            style={{ width: 64 }}
          />
        </SettingRow>
      </div>
      {keyConflict && (
        <p className="ol-dictation-reference-conflict" role="alert">
          {t('settings.dictationReferences.keyConflict')}
        </p>
      )}
      {enabled && screenCapture === 'denied' && (
        <div className="ol-dictation-reference-permission">
          <strong>{t('settings.dictationReferences.permissionNote')}</strong>
          <Button
            className="ol-dictation-reference-settings-button"
            onClick={async () => {
              if (!(await openDictationReferencesPermissionSettings())) onPermissionSettingsUnavailable();
            }}
          >
            {t('settings.dictationReferences.openSettings')}
          </Button>
        </div>
      )}
    </Card>
  );
}
