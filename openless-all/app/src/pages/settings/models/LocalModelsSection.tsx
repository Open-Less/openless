// Model settings page (2.0): download and management dashboard for local ASR models.
// Covers three local engines: Qwen3 (macOS) / Foundry Local + sherpa-onnx (Windows).
//
// Local ASR no longer has an "enable toggle" — activation is decided by the ASR speech
// transcription provider under Services → AI Providers: picking a local model provider uses the
// local engine (same idea as Apple Speech). This page is the model catalog and management
// (download / delete / clean leftovers / test / mirror source); switching the model in use
// happens in the channel editor's LocalModelPicker.

import { useEffect, useState } from 'react';
import type { PlatformCapabilities } from '../../../lib/types';
import { useTranslation } from 'react-i18next';
import { LocalAsr } from '../../LocalAsr';
import { detectOS } from '../../../components/WindowChrome';
import { getPlatformCapabilities } from '../../../lib/platform';
import { Card } from '../../_atoms';
import { ExperimentalSectionTitle } from '../shared';

export function LocalModelsSection() {
  const { t } = useTranslation();
  const os = detectOS();
  const isWin = os === 'win';
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  const platformSupported = platformCaps?.supportsLocalAsr === true;

  return (
    <Card>
      {/* Title + inline warning text in the top-right corner (experimental badge kept).
          Windows: the title area is grayed out — local ASR on Windows goes through the
          separate Foundry / sherpa paths. */}
      <div
        style={{
          display: 'flex',
          alignItems: 'flex-start',
          justifyContent: 'space-between',
          gap: 12,
          marginBottom: 14,
        }}
      >
        <div style={{ minWidth: 0, opacity: isWin ? 0.45 : 1 }}>
          <ExperimentalSectionTitle badge={t('common.experimental')} style={{ marginBottom: 0 }}>
            {t('settings.advanced.localAsrTitle')}
          </ExperimentalSectionTitle>
        </div>
        <div
          style={{
            fontSize: 11,
            color: '#A04500',
            fontWeight: 500,
            lineHeight: 1.4,
            textAlign: 'right',
            flexShrink: 0,
            maxWidth: '52%',
            paddingTop: 2,
            opacity: isWin ? 0.45 : 1,
          }}
        >
          ⚠️ {t('settings.advanced.localAsrWarningShort')}
        </div>
      </div>

      {!platformSupported ? (
        <div
          style={{ fontSize: 12.5, color: 'var(--ol-ink-3)', lineHeight: 1.6, padding: '8px 0' }}
        >
          {t('settings.advanced.platformNotSupported')}
        </div>
      ) : (
        /* Model catalog / management dashboard (model selection · download · cleanup · delete · test · mirror source).
           Usage entry: pick a local provider under "AI Providers → ASR speech transcription",
           then switch the model in use via the LocalModelPicker in the channel editor. */
        <div style={{ marginTop: 16, borderTop: '0.5px solid var(--ol-line)', paddingTop: 16 }}>
          <LocalAsr embedded />
        </div>
      )}
    </Card>
  );
}
