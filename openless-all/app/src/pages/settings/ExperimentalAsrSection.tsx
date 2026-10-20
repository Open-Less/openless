import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { PresenceModal } from '../../components/ui/Modal';
import { Collapse } from '../../components/ui/Disclosure';
import { Icon } from '../../components/Icon';
import { Btn, Card } from '../_atoms';
import { ExperimentalSectionTitle, SettingRow, Toggle } from './shared';
import {
  downloadPercent,
  EXPERIMENTAL_ASR_MODELS,
  type ExperimentalAsrStatus,
  experimentalAsrName,
  needsModelGuidance,
} from '../../lib/experimentalAsr';

export { useExperimentalAsr } from '../../lib/useExperimentalAsr';
import type { ExperimentalAsrController } from '../../lib/useExperimentalAsr';
export type { ExperimentalAsrController } from '../../lib/useExperimentalAsr';
export function ExperimentalAsrSection({
  controller,
  onOpenAsr,
}: {
  controller: ExperimentalAsrController;
  onOpenAsr: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const { models, error, pending, toggle, remove } = controller;
  const [navigationError, setNavigationError] = useState<string | null>(null);
  const loading = models.length === 0;
  // Reserve the three rows before the first host response. Recommendation and
  // downstream controls must not suddenly appear below a one-line placeholder.
  const rows: ExperimentalAsrStatus[] = loading
    ? EXPERIMENTAL_ASR_MODELS.map((model) => ({
        model,
        enabled: false,
        available: false,
        unavailableReason: null,
        phase: 'disabled',
        downloadedBytes: 0,
        totalBytes: 0,
        channelId: null,
        needsGuidance: false,
        error: null,
      }))
    : models;
  return (
    <Card className="ol-experimental-models">
      <ExperimentalSectionTitle badge={t('common.experimental')}>
        {t('experimentalAsr.title')}
      </ExperimentalSectionTitle>
      <Collapse open={!!navigationError || !!error}>
        <p role="alert" style={{ color: 'var(--ol-warn)' }}>
          {navigationError ?? error}
        </p>
      </Collapse>
      {rows.map((status) => {
        const name = experimentalAsrName(status.model);
        const percent = downloadPercent(status);
        return (
          <section key={status.model} data-experimental-model={status.model}>
            <SettingRow
              className="ol-experimental-model-row"
              label={
                status.model === 'text-stream'
                  ? `${name} · ${t('experimentalAsr.recommended')}`
                  : name
              }
              desc={
                <>
                  <p>
                    {t(
                      status.model === 'text-stream'
                        ? 'experimentalAsr.textStreamDescription'
                        : status.model === 'sense-voice'
                          ? 'experimentalAsr.senseVoiceDescription'
                          : 'experimentalAsr.gemmaDescription',
                    )}
                  </p>
                  <p>{t('experimentalAsr.description')}</p>
                  <p>{t('experimentalAsr.memoryHint')}</p>
                  {status.model === 'text-stream' && <p>{t('experimentalAsr.streamHint')}</p>}
                  <p>{t('experimentalAsr.deleteHint')}</p>
                  <p>{t('experimentalAsr.pipelineHint')}</p>
                  {!status.available && (
                    <p>
                      {t(`experimentalAsr.${status.unavailableReason ?? 'desktopRuntimeRequired'}`)}
                    </p>
                  )}
                </>
              }
            >
              <div className="ol-model-state">
                <span
                  role="status"
                  aria-live="polite"
                  className={
                    status.phase === 'disabled' || status.phase === 'ready'
                      ? 'ol-sr-only'
                      : 'ol-experimental-model-status'
                  }
                >
                  {loading ? t('common.loading') : t(`experimentalAsr.phases.${status.phase}`)}
                  {status.phase === 'downloading' &&
                    status.downloadedBytes > 0 &&
                    ` · ${percent || '<1'}%`}
                </span>
                {status.phase === 'downloading' && (
                  <progress
                    aria-label={t('experimentalAsr.downloadProgress', { name })}
                    value={status.downloadedBytes > 0 ? percent : undefined}
                    max={100}
                  />
                )}
              </div>
              <div className="ol-model-action">
                {status.phase === 'ready' && (
                  <button
                    type="button"
                    className="ol-model-channel-link"
                    title={t('experimentalAsr.openChannels')}
                    aria-label={t('experimentalAsr.openChannels')}
                    onClick={() => void onOpenAsr().catch((e) => setNavigationError(String(e)))}
                  >
                    <Icon name="chevRight" size={17} />
                  </button>
                )}
              </div>
              <Toggle
                label={name}
                on={status.enabled}
                disabled={
                  loading ||
                  pending.includes(status.model) ||
                  status.phase === 'removing' ||
                  (!status.available && !status.enabled)
                }
                onToggle={(next) => void toggle(status.model, next).catch(() => {})}
              />
              {status.phase === 'failed' && (
                <Btn
                  size="sm"
                  variant="ghost"
                  onClick={() => void toggle(status.model, status.enabled).catch(() => {})}
                  disabled={pending.includes(status.model)}
                >
                  {t('experimentalAsr.retry')}
                </Btn>
              )}
              {!status.enabled && status.downloadedBytes > 0 && (
                <Btn
                  size="sm"
                  variant="ghost"
                  disabled={pending.includes(status.model) || status.phase === 'removing'}
                  onClick={() => {
                    if (window.confirm(t('localAsr.deleteConfirm', { name })))
                      void remove(status.model).catch(() => {});
                  }}
                >
                  {t('localAsr.delete')}
                </Btn>
              )}
            </SettingRow>
            <Collapse open={!!status.error}>
              <p
                role="alert"
                style={{ fontSize: 12, color: 'var(--ol-warn)', overflowWrap: 'anywhere' }}
              >
                {status.error}
              </p>
            </Collapse>
          </section>
        );
      })}
    </Card>
  );
}

export function ExperimentalAsrGuidance({
  controller,
  onOpenAsr,
}: {
  controller: ExperimentalAsrController;
  onOpenAsr: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ready = controller.models.filter(needsModelGuidance);
  const close = async (navigate: boolean) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      if (navigate) await onOpenAsr();
      await controller.acknowledge(ready);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <PresenceModal
      open={ready.length > 0}
      zIndex={1000}
      labelledBy="experimental-asr-ready-title"
      onClose={() => void close(false)}
      width="min(460px, calc(100vw - 32px))"
      render={() => (
        <>
          <h2 id="experimental-asr-ready-title" style={{ fontSize: 17 }}>
            {t('experimentalAsr.readyTitle')}
          </h2>
          <p>{ready.map((s) => experimentalAsrName(s.model)).join(' / ')}</p>
          <p style={{ fontSize: 13, lineHeight: 1.7, color: 'var(--ol-ink-3)' }}>
            {t('experimentalAsr.readyBody')}
          </p>
          <p style={{ fontSize: 12, lineHeight: 1.6, color: 'var(--ol-ink-3)' }}>
            {t('experimentalAsr.pipelineHint')}
          </p>
          {error && <p role="alert">{error}</p>}
          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 20 }}>
            <Btn variant="ghost" disabled={busy} onClick={() => void close(false)}>
              {t('experimentalAsr.later')}
            </Btn>
            <Btn disabled={busy} onClick={() => void close(true)}>
              {t('experimentalAsr.confirm')}
            </Btn>
          </div>
        </>
      )}
    />
  );
}
