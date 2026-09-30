// Model usage page: embedded model picker for the channel editor's "local engine" branch.
// The global active model and engine status come from the local ASR IPC — local models are not isolated
// per channel, so this switches the globally active model; management actions like download / delete /
// mirror stay in "Services → Local models" (jump via useLocalModelsNav).

import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { SelectLite } from '../../../components/ui/SelectLite';
import { Btn } from '../../_atoms';
import { SettingRow, inputStyle } from '../shared';
import {
  cleanupIncompleteLocalAsrModel,
  getFoundryLocalAsrCatalog,
  getFoundryLocalAsrStatus,
  getLocalAsrSettings,
  getSherpaOnnxAsrCatalog,
  getSherpaOnnxAsrStatus,
  listLocalAsrModels,
  preloadLocalAsr,
  setFoundryLocalAsrModel,
  setLocalAsrActiveModel,
  setSherpaOnnxAsrModel,
  type FoundryLocalAsrCatalogModel,
  type LocalAsrModelStatus,
  type SherpaOnnxCatalogModel,
} from '../../../lib/localAsr';
import { emitSaved } from '../../../lib/savedEvent';
import { useLocalModelsNav } from './modelsNav';

/** Local engine providerType → picker data-source family. */
function familyOfProviderType(providerType: string): 'generic' | 'sherpa' | 'foundry' | 'apple' {
  if (providerType === 'sherpa-onnx-local') return 'sherpa';
  if (providerType === 'foundry-local-whisper') return 'foundry';
  if (providerType === 'apple-speech') return 'apple';
  return 'generic';
}

interface ModelOption {
  value: string;
  label: string;
  downloaded: boolean;
  /** Downloaded bytes but not installed = interrupted leftovers, cleanable with one click. */
  partialBytes?: number;
}

export function LocalModelPicker({ providerType }: { providerType: string }) {
  const { t } = useTranslation();
  const openModels = useLocalModelsNav();
  const family = familyOfProviderType(providerType);

  const [options, setOptions] = useState<ModelOption[] | null>(null);
  const [activeModel, setActiveModel] = useState('');
  const [engineNote, setEngineNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      if (family === 'generic') {
        const [settings, models] = await Promise.all([getLocalAsrSettings(), listLocalAsrModels()]);
        setOptions(
          models.map((m: LocalAsrModelStatus) => ({
            value: m.id,
            label: m.displayName || m.id,
            downloaded: m.isDownloaded,
            partialBytes: !m.isDownloaded && m.downloadedBytes > 0 ? m.downloadedBytes : undefined,
          })),
        );
        setActiveModel(settings.activeModel);
        setEngineNote(settings.engineAvailable ? null : t('localAsr.engineUnavailable'));
      } else if (family === 'sherpa') {
        const [status, catalog] = await Promise.all([
          getSherpaOnnxAsrStatus(),
          getSherpaOnnxAsrCatalog(),
        ]);
        setOptions(
          catalog.map((c: SherpaOnnxCatalogModel) => ({
            value: c.alias,
            label: c.displayName || c.alias,
            downloaded: c.cached,
          })),
        );
        setActiveModel(status.activeModel);
        setEngineNote(status.available ? null : (status.error ?? t('localAsr.engineUnavailable')));
      } else if (family === 'foundry') {
        const [status, catalog] = await Promise.all([
          getFoundryLocalAsrStatus(),
          getFoundryLocalAsrCatalog(),
        ]);
        setOptions(
          catalog.map((c: FoundryLocalAsrCatalogModel) => ({
            value: c.alias,
            label: c.displayName || c.alias,
            downloaded: c.cached,
          })),
        );
        setActiveModel(status.activeModel);
        setEngineNote(status.available ? null : (status.error ?? t('localAsr.engineUnavailable')));
      }
    } catch (error) {
      console.error('[local-models] failed to load picker state', error);
      setEngineNote(t('settings.providers.readFailed'));
    }
  }, [family, t]);

  useEffect(() => {
    void load();
  }, [load]);

  const partials = useMemo(
    () => (options ?? []).filter((option) => option.partialBytes),
    [options],
  );

  const changeModel = async (value: string) => {
    setBusy(true);
    try {
      if (family === 'generic') {
        await setLocalAsrActiveModel(value);
        await preloadLocalAsr().catch(() => undefined);
      } else if (family === 'sherpa') {
        await setSherpaOnnxAsrModel(value);
      } else if (family === 'foundry') {
        await setFoundryLocalAsrModel(value);
      }
      emitSaved('saved', t('common.saved'));
      await load();
    } catch (error) {
      console.error('[local-models] failed to switch active model', error);
      emitSaved('failed', t('common.operationFailed'));
    } finally {
      setBusy(false);
    }
  };

  const cleanupPartial = async (modelId: string) => {
    setBusy(true);
    try {
      await cleanupIncompleteLocalAsrModel(modelId);
      emitSaved('saved', t('common.saved'));
      await load();
    } catch (error) {
      console.error('[local-models] failed to cleanup incomplete download', error);
      emitSaved('failed', t('common.operationFailed'));
    } finally {
      setBusy(false);
    }
  };

  if (family === 'apple') {
    return (
      <p className="ol-channel-local-hint">{t('settings.providers.localEngineNoCredentials')}</p>
    );
  }

  const downloadedCount = (options ?? []).filter((option) => option.downloaded).length;

  return (
    <>
      <SettingRow label={t('localAsr.activeModelLabel')}>
        {options === null ? (
          <span style={{ fontSize: 12, color: 'var(--ol-ink-3)' }}>{t('common.loading')}</span>
        ) : downloadedCount === 0 ? (
          <span style={{ fontSize: 12, color: 'var(--ol-ink-3)' }}>
            {t('localAsr.pickerNoModelDownloaded')}
          </span>
        ) : (
          <SelectLite
            value={activeModel}
            disabled={busy}
            onChange={(value) => void changeModel(value)}
            options={(options ?? [])
              .filter((option) => option.downloaded)
              .map((option) => ({ value: option.value, label: option.label }))}
            ariaLabel={t('localAsr.activeModelLabel')}
            style={{ ...inputStyle, width: '100%', maxWidth: '100%', height: 38 }}
          />
        )}
      </SettingRow>

      {partials.length > 0 && (
        <SettingRow
          label={t('localAsr.partialDownloadsLabel')}
          desc={t('localAsr.partialDownloadsDesc')}
        >
          <Btn
            variant="ghost"
            size="sm"
            disabled={busy}
            onClick={() => {
              for (const option of partials) void cleanupPartial(option.value);
            }}
          >
            {t('localAsr.cleanupIncomplete')}
          </Btn>
        </SettingRow>
      )}

      {engineNote && (
        <p role="alert" style={{ fontSize: 12, color: 'var(--ol-warn)' }}>
          {engineNote}
        </p>
      )}

      {openModels && (
        <div style={{ display: 'flex', justifyContent: 'flex-end', marginTop: 2 }}>
          <Btn variant="ghost" size="sm" onClick={openModels}>
            {t('settings.providers.localAsrManage')}
          </Btn>
        </div>
      )}
    </>
  );
}
