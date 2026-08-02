// 高级 → 调试工具：保留原始录音、导出错误日志等排障入口。
// recordAudioForDebug 行自 Settings.tsx 的 RecordingSection 拆出；
// 导出错误日志自 SettingsModal 的 AboutMini 迁入 —— 调试相关集中到此处。

import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { debugReadCursorContext, exportErrorLog } from '../../lib/ipc';
import { useMobileLayout } from '../../lib/useMobileLayout';
import type { HostDocumentReadResult } from '../../lib/types';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Btn, Card } from '../_atoms';
import { SettingRow, Toggle, SectionTitle, inputStyle } from './shared';

const clamp = (n: number, min: number, max: number) => Math.max(min, Math.min(max, n));

export function DebugToolsSection() {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  const { prefs, updatePrefs: savePrefs } = useHotkeySettings();
  const [exportStatus, setExportStatus] = useState<'idle' | 'busy' | 'ok' | 'err'>('idle');
  const [exportMessage, setExportMessage] = useState<string>('');
  const exportTimerRef = useRef<number | null>(null);
  // 光标上下文探针。倒计时让用户有时间切到目标 app —— 见 onProbeCursorContext。
  const [probeCountdown, setProbeCountdown] = useState(0);
  const [probeResult, setProbeResult] = useState<HostDocumentReadResult | null>(null);
  const [probeError, setProbeError] = useState<string | null>(null);
  const probeTimerRef = useRef<number | null>(null);

  useEffect(() => () => {
    if (exportTimerRef.current) clearTimeout(exportTimerRef.current);
    if (probeTimerRef.current) clearInterval(probeTimerRef.current);
  }, []);

  /// 点一下 → 倒数几秒 → 读一次前台 app 的光标上下文。
  ///
  /// 必须有倒计时：点按钮的那一刻前台 app 是 OpenLess 自己，直接读只会读到我们自己的
  /// 设置窗口。倒计时期间切到备忘录 / VS Code / 微信里点进输入框，探针才读得到真东西。
  const PROBE_DELAY_SECONDS = 5;
  const onProbeCursorContext = async () => {
    setProbeResult(null);
    setProbeError(null);
    setProbeCountdown(PROBE_DELAY_SECONDS);
    if (probeTimerRef.current) clearInterval(probeTimerRef.current);
    probeTimerRef.current = window.setInterval(() => {
      setProbeCountdown(prev => {
        if (prev <= 1 && probeTimerRef.current) clearInterval(probeTimerRef.current);
        return Math.max(0, prev - 1);
      });
    }, 1000);
    try {
      setProbeResult(await debugReadCursorContext(PROBE_DELAY_SECONDS * 1000));
    } catch (err) {
      setProbeError(err instanceof Error ? err.message : String(err));
    } finally {
      setProbeCountdown(0);
      if (probeTimerRef.current) clearInterval(probeTimerRef.current);
    }
  };

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
        <div style={{ display: 'flex', flexDirection: mobile ? 'column' : 'row', gap: 8, alignItems: mobile ? 'stretch' : 'center', width: '100%' }}>
          <input
            type="number"
            min={1}
            max={200}
            placeholder="200"
            value={prefs.audioRecordingMaxEntries ?? ''}
            onChange={e => onAudioRecordingMaxEntriesChange(e.target.value)}
            style={{ ...inputStyle, width: mobile ? '100%' : 80, textAlign: 'right' }}
            disabled={!prefs.recordAudioForDebug}
          />
          {mobile && (
            <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', lineHeight: 1.45 }}>
              {t('settings.recording.audioRecordingMaxEntriesDesc')}
            </span>
          )}
        </div>
      </SettingRow>
      {/* 光标上下文探针。里程碑 1 的产物「能肉眼看它在各 app 里读到了什么」——
          没有这个入口，那条命令就等于不存在。 */}
      <SettingRow label={t('settings.debug.cursorProbeLabel')} desc={t('settings.debug.cursorProbeDesc')}>
        <div style={{ display: 'grid', gap: 6, minWidth: 0 }}>
          <div>
            <Btn variant="ghost" size="sm" disabled={probeCountdown > 0} onClick={() => void onProbeCursorContext()}>
              {probeCountdown > 0
                ? t('settings.debug.cursorProbeCountdown', { n: probeCountdown })
                : t('settings.debug.cursorProbeBtn')}
            </Btn>
          </div>
          {probeError && (
            <div style={{ fontSize: 11, color: 'var(--ol-err)' }}>{probeError}</div>
          )}
          {probeResult && (
            <div
              style={{
                fontSize: 11, fontFamily: 'var(--ol-font-mono)', lineHeight: 1.7,
                padding: '8px 10px', borderRadius: 8,
                background: 'var(--ol-surface-2)',
                border: '0.5px solid var(--ol-line-strong)',
                maxWidth: 420, wordBreak: 'break-word',
              }}
            >
              <div>
                <b>{probeResult.status}</b>
                {probeResult.reason ? ` — ${probeResult.reason}` : ''}
                {` · ${probeResult.elapsedMs}ms`}
              </div>
              <div style={{ color: 'var(--ol-ink-4)' }}>
                {probeResult.appName ?? '?'} ({probeResult.bundleId ?? '?'})
              </div>
              {probeResult.window && (
                <div style={{ marginTop: 4, whiteSpace: 'pre-wrap' }}>
                  {probeResult.window.text.slice(0, probeResult.window.cursor)}
                  <span style={{ color: 'var(--ol-blue)', fontWeight: 700 }}>⟦光标⟧</span>
                  {probeResult.window.text.slice(probeResult.window.cursor)}
                </div>
              )}
            </div>
          )}
        </div>
      </SettingRow>
      <SettingRow label={t('modal.about.exportErrorLog')}>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
          <Btn variant="ghost" size="sm" disabled={exportStatus === 'busy'} onClick={onExportLog}>
            {exportStatus === 'busy' ? t('modal.about.exporting') : t('modal.about.exportErrorLogBtn')}
          </Btn>
          {exportStatus === 'ok' && (
            <span
              style={{
                fontSize: 11,
                color: 'var(--ol-ok)',
                whiteSpace: mobile ? 'normal' : 'nowrap',
                overflow: mobile ? 'visible' : 'hidden',
                textOverflow: mobile ? 'clip' : 'ellipsis',
                wordBreak: 'break-word',
                maxWidth: mobile ? '100%' : 220,
              }}
              title={exportMessage}
            >
              {t('modal.about.exportSuccess')}
              {mobile && exportMessage ? `：${exportMessage}` : ''}
            </span>
          )}
          {exportStatus === 'err' && (
            <span
              style={{ fontSize: 11, color: 'var(--ol-err)', lineHeight: 1.45, wordBreak: 'break-word', maxWidth: mobile ? '100%' : 280 }}
              title={exportMessage}
            >
              {t('modal.about.exportFailed')}
              {exportMessage ? `：${exportMessage}` : ''}
            </span>
          )}
        </div>
      </SettingRow>
    </Card>
  );
}
