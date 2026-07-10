// 高级 → 调试工具：保留原始录音、导出错误日志等排障入口。
// recordAudioForDebug 行自 Settings.tsx 的 RecordingSection 拆出；
// 导出错误日志自 SettingsModal 的 AboutMini 迁入 —— 调试相关集中到此处。

import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { exportErrorLog } from '../../lib/ipc';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Btn, Card } from '../_atoms';
import { SettingRow, Toggle, SectionTitle, inputStyle } from './shared';

const clamp = (n: number, min: number, max: number) => Math.max(min, Math.min(max, n));

export function DebugToolsSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs: savePrefs } = useHotkeySettings();
  const [exportStatus, setExportStatus] = useState<'idle' | 'busy' | 'ok' | 'err'>('idle');
  const [exportMessage, setExportMessage] = useState<string>('');
  const exportTimerRef = useRef<number | null>(null);

  useEffect(() => () => {
    if (exportTimerRef.current) clearTimeout(exportTimerRef.current);
  }, []);

  const onExportLog = async () => {
    setExportStatus('busy');
    setExportMessage('');
    try {
      const ts = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
      const target = await exportErrorLog(`openless-${ts}.log`);
      if (target == null) {
        setExportStatus('idle');
        return;
      }
      setExportStatus('ok');
      setExportMessage(target);
      if (exportTimerRef.current) clearTimeout(exportTimerRef.current);
      exportTimerRef.current = window.setTimeout(() => setExportStatus('idle'), 4000);
    } catch (err) {
      setExportStatus('err');
      setExportMessage(err instanceof Error ? err.message : String(err));
    }
  };

  if (!prefs) {
    return (
      <Card>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
      </Card>
    );
  }

  const onRecordAudioForDebugChange = (recordAudioForDebug: boolean) =>
    savePrefs({ ...prefs, recordAudioForDebug });
  // 留空视为不限制，落回 null → 后端走 200 默认。
  const onAudioRecordingMaxEntriesChange = (raw: string) => {
    const trimmed = raw.trim();
    if (trimmed === '') {
      void savePrefs({ ...prefs, audioRecordingMaxEntries: null });
      return;
    }
    const parsed = Number.parseInt(trimmed, 10);
    if (Number.isNaN(parsed)) return;
    void savePrefs({ ...prefs, audioRecordingMaxEntries: clamp(parsed, 1, 200) });
  };

  return (
    <Card>
      <SectionTitle>{t('settings.debug.title')}</SectionTitle>
      <SettingRow label={t('settings.recording.recordAudioForDebugLabel')}>
        <Toggle on={prefs.recordAudioForDebug} onToggle={onRecordAudioForDebugChange} />
      </SettingRow>
      <SettingRow label={t('settings.recording.audioRecordingMaxEntriesLabel')}>
        <input
          type="number"
          min={1}
          max={200}
          placeholder="200"
          value={prefs.audioRecordingMaxEntries ?? ''}
          onChange={e => onAudioRecordingMaxEntriesChange(e.target.value)}
          style={{ ...inputStyle, width: 80, textAlign: 'right' }}
          disabled={!prefs.recordAudioForDebug}
        />
      </SettingRow>
      <SettingRow label={t('modal.about.exportErrorLog')}>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <Btn variant="ghost" size="sm" disabled={exportStatus === 'busy'} onClick={onExportLog}>
            {exportStatus === 'busy' ? t('modal.about.exporting') : t('modal.about.exportErrorLogBtn')}
          </Btn>
          {exportStatus === 'ok' && (
            <span
              style={{ fontSize: 11, color: 'var(--ol-ok)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', maxWidth: 220 }}
              title={exportMessage}
            >
              {t('modal.about.exportSuccess')}
            </span>
          )}
          {exportStatus === 'err' && (
            <span
              style={{ fontSize: 11, color: 'var(--ol-err)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', maxWidth: 220 }}
              title={exportMessage}
            >
              {t('modal.about.exportFailed')}
            </span>
          )}
        </div>
      </SettingRow>
    </Card>
  );
}
