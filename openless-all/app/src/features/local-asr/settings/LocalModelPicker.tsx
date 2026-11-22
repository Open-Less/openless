// Model usage page: embedded model picker for the channel editor's "local engine" branch.
// The global active model and engine status come from the local ASR IPC — local models are not isolated
// per channel, so this switches the globally active model; management actions like download / delete /
// mirror stay in "Services → Local models" (jump via useLocalModelsNav).

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { createDeletionBatch, IncompleteDeletion } from '../../../api/deletion-batch';
import { useInteraction } from '../../../ui/overlays/InteractionProvider';
import { SelectLite } from '../../../components/ui/SelectLite';
import { Button as Btn } from '../../../ui/controls/Button';
import { SettingRow } from '../../../ui/controls/SettingRow';
import { inputStyle } from '../../../ui/controls/settingsFieldStyles';
import {
  cleanupIncompleteLocalAsrModel,
  getFoundryLocalAsrCatalog,
  getFoundryLocalAsrStatus,
  getLocalAsrSettings,
  getSherpaOnnxAsrCatalog,
  getSherpaOnnxAsrStatus,
  listLocalAsrModels,
  setFoundryLocalAsrModel,
  setLocalAsrActiveModel,
  setSherpaOnnxAsrModel,
  type FoundryLocalAsrCatalogModel,
  type LocalAsrModelStatus,
  type SherpaOnnxCatalogModel,
} from '../../../api/local-asr';
import { emitSaved } from '../../../lib/savedEvent';
import { useLocalModelsNav } from './modelsNav';
import { isExperimentalAsrProvider, isRetiredMacAsrProvider } from '../../../lib/experimentalAsr';
import { getRuntimeOS } from '../../../runtime/environment';

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
  /** Downloaded bytes but not installed = interrupted leftovers available for cleanup. */
  partialBytes?: number;
}

export function LocalModelPicker({ providerType }: { providerType: string }) {
  const { t } = useTranslation();
  const { confirm, notify } = useInteraction(providerType);
  const deleteRequest = useRef(false);
  const openModels = useLocalModelsNav();
  const experimental = isExperimentalAsrProvider(providerType);
  const retired = getRuntimeOS() === 'mac' && isRetiredMacAsrProvider(providerType);
  const family = familyOfProviderType(providerType);

  const [options, setOptions] = useState<ModelOption[] | null>(null);
  const [activeModel, setActiveModel] = useState('');
  const [engineNote, setEngineNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      if (experimental || retired) return;
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
  }, [family, t, experimental, retired]);

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

  const cleanupPartials = async () => {
    if (deleteRequest.current || !partials.length) return;
    deleteRequest.current = true;
    const remove = createDeletionBatch(
      partials.map((option) => option.value),
      cleanupIncompleteLocalAsrModel,
    );
    try {
      await confirm({
        title: t('localAsr.cleanupIncomplete'),
        description: t('localAsr.cleanupBatchScope', { count: partials.length }),
        confirmLabel: t('common.delete'),
        tone: 'danger',
        icon: 'trash',
        errorMessage: (error) =>
          error instanceof IncompleteDeletion
            ? t('localAsr.cleanupIncompleteError', { count: error.remaining })
            : t('common.operationFailed'),
        onConfirm: async () => {
          setBusy(true);
          try {
            await remove((removed) =>
              setOptions(
                (current) =>
                  current?.map((option) =>
                    removed.has(option.value) ? { ...option, partialBytes: undefined } : option,
                  ) ?? null,
              ),
            );
            notify({ state: 'deleted', message: t('common.deleted') });
          } finally {
            await load();
            setBusy(false);
          }
        },
      });
    } finally {
      deleteRequest.current = false;
    }
  };

  if (retired) return <p>{t('experimentalAsr.retiredHint')}</p>;
  if (experimental) return <p>{t('experimentalAsr.manageHint')}</p>;

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
          <Btn variant="ghost" size="sm" disabled={busy} onClick={() => void cleanupPartials()}>
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
