// Remote input: on the LAN, a phone/tablet browser opens a recording page and streams voice
// back to the PC in real time, reusing the existing "record -> ASR -> polish -> insert at
// cursor" pipeline. This is already a second-level page, so the controls stay expanded:
// start/stop switch, listening port, access URLs (one-click copy, with pairing
// code), pairing code (resettable), default recording mode, and certificate/security notes.

import { useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import { useHotkeySettings } from '../../data/preferences';
import { Segmented } from '../../components/ui/Segmented';
import { Card } from '../../pages/_atoms';
import { SectionTitle, SettingRow, Toggle, inputStyle } from '../../pages/settings/shared';
import { regenerateRemotePin } from '../../api/input';
import { useRemoteInputStatus } from './useRemoteInputStatus';
import { getRemoteInputViewState } from './remoteInputViewState';

async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    return;
  } catch {
    // Fallback: hidden textarea + execCommand, for environments without async clipboard support.
    const ta = document.createElement('textarea');
    ta.value = text;
    ta.style.position = 'fixed';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    ta.select();
    try {
      document.execCommand('copy');
    } catch {
      /* ignore */
    }
    document.body.removeChild(ta);
  }
}

export function RemoteInputSection() {
  const { t, i18n } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const { status, startError, acceptStatus } = useRemoteInputStatus(i18n.language);
  const [copied, setCopied] = useState<string | null>(null);
  // Port edit draft: parsed and committed on blur/Enter only, so per-keystroke persistence does
  // not restart the backend service repeatedly on intermediate port values.
  const [portDraft, setPortDraft] = useState<string | null>(null);

  if (!prefs) return null;
  const enabled = prefs.remoteInputEnabled;
  const mode = prefs.remoteInputDefaultMode ?? 'toggle';
  const viewState = getRemoteInputViewState(enabled, status, startError);
  // Only a full fingerprint returned by the local listener qualifies for phone verification.
  const fingerprint = status?.caFingerprintSha256;
  const canVerifyCertificate =
    typeof fingerprint === 'string' && /^[a-f0-9]{64}$/i.test(fingerprint);
  const formattedFingerprint = canVerifyCertificate
    ? fingerprint.toUpperCase().match(/.{2}/g)!.join(' ')
    : '';

  // Commit the port draft: invalid (non-finite / wildly out of range) discards and restores the
  // display; valid rounds and clamps into [1024, 65535].
  const commitPort = () => {
    if (portDraft == null) return;
    const n = Math.round(Number(portDraft));
    if (!Number.isFinite(n) || n <= 0) {
      setPortDraft(null);
      return;
    }
    const port = Math.max(1024, Math.min(65535, n));
    setPortDraft(null);
    if (port !== prefs.remoteInputPort) {
      updatePrefs({ ...prefs, remoteInputPort: port });
    }
  };

  const doCopy = async (url: string, pin: string) => {
    await copyText(`${url}\n${t('settings.remoteInput.pinLabel')}：${pin}`);
    setCopied(url);
    window.setTimeout(() => setCopied((c) => (c === url ? null : c)), 1500);
  };

  const smallBtn: CSSProperties = {
    padding: '4px 10px',
    borderRadius: 'var(--ol-control-radius)',
    fontSize: 12,
    cursor: 'pointer',
    border: '0.5px solid var(--ol-line-strong)',
    background: 'var(--ol-surface-2)',
    color: 'var(--ol-ink)',
    flexShrink: 0,
  };

  return (
    <Card>
      <SectionTitle>{t('settings.remoteInput.title')}</SectionTitle>
      <SettingRow
        label={t('settings.remoteInput.enableLabel')}
        desc={t('settings.remoteInput.enableDesc')}
      >
        <Toggle on={enabled} onToggle={(v) => updatePrefs({ ...prefs, remoteInputEnabled: v })} />
      </SettingRow>

      <SettingRow label={t('settings.remoteInput.portLabel')}>
        <input
          type="number"
          min={1024}
          max={65535}
          style={{ ...inputStyle, maxWidth: 140 }}
          value={portDraft ?? String(prefs.remoteInputPort)}
          onChange={(e) => setPortDraft(e.currentTarget.value)}
          onBlur={commitPort}
          onKeyDown={(e) => {
            if (e.key === 'Enter') commitPort();
          }}
        />
      </SettingRow>

      <SettingRow label={t('settings.remoteInput.defaultModeLabel')}>
        <Segmented
          label={t('settings.remoteInput.defaultModeLabel')}
          value={mode}
          onChange={(m) => updatePrefs({ ...prefs, remoteInputDefaultMode: m })}
          options={(['toggle', 'hold'] as const).map((m) => ({
            value: m,
            label: t(
              m === 'toggle' ? 'settings.remoteInput.modeToggle' : 'settings.remoteInput.modeHold',
            ),
          }))}
        />
      </SettingRow>

      {enabled && (viewState === 'running' || viewState === 'stale') && status && (
        <>
          <SettingRow label={t('settings.remoteInput.certFingerprintLabel')}>
            <div style={{ minWidth: 0, width: '100%' }}>
              {canVerifyCertificate ? (
                <>
                  <code
                    aria-label={t('settings.remoteInput.certFingerprintLabel')}
                    style={{
                      display: 'block',
                      fontSize: 12,
                      lineHeight: 1.8,
                      overflowWrap: 'anywhere',
                      userSelect: 'text',
                    }}
                  >
                    {formattedFingerprint}
                  </code>
                  <button
                    type="button"
                    onClick={async () => {
                      await copyText(formattedFingerprint);
                      setCopied('ca-fingerprint');
                      window.setTimeout(
                        () => setCopied((c) => (c === 'ca-fingerprint' ? null : c)),
                        1500,
                      );
                    }}
                    style={{ ...smallBtn, marginTop: 6 }}
                  >
                    {copied === 'ca-fingerprint'
                      ? t('settings.remoteInput.certFingerprintCopied')
                      : t('settings.remoteInput.certFingerprintCopy')}
                  </button>
                </>
              ) : (
                <p role="alert" style={{ color: 'var(--ol-red)', margin: 0 }}>
                  {t('settings.remoteInput.certFingerprintUnavailable')}
                </p>
              )}
              <p style={{ fontSize: 12, lineHeight: 1.6, margin: '8px 0 0' }}>
                {t('settings.remoteInput.certVerifyHint')}
              </p>
              <p style={{ fontSize: 12, lineHeight: 1.6, margin: '6px 0 0' }}>
                {t('settings.remoteInput.certProfileHint')}
              </p>
            </div>
          </SettingRow>
          {status.urls.length > 0 && (
            <SettingRow label={t('settings.remoteInput.urlLabel')}>
              <div
                style={{
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 6,
                  minWidth: 0,
                }}
              >
                {status.urls.map((u) => (
                  <div
                    key={u}
                    style={{ display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: 8 }}
                  >
                    <span
                      style={{
                        fontFamily: 'monospace',
                        fontSize: 12.5,
                        color: 'var(--ol-ink-2)',
                        wordBreak: 'break-all',
                      }}
                    >
                      {u}
                    </span>
                    <button
                      onClick={() => doCopy(u, status.pin)}
                      title={t('settings.remoteInput.urlLabel')}
                      style={smallBtn}
                    >
                      {copied === u ? '✓' : '⧉'}
                    </button>
                    <button
                      disabled={!canVerifyCertificate}
                      onClick={async () => {
                        const link = `${u}/cert.mobileconfig`;
                        await copyText(link);
                        setCopied(link);
                        window.setTimeout(() => setCopied((c) => (c === link ? null : c)), 1500);
                      }}
                      style={smallBtn}
                    >
                      {copied === `${u}/cert.mobileconfig`
                        ? '✓'
                        : t('settings.remoteInput.certSetupLink')}
                    </button>
                  </div>
                ))}
              </div>
            </SettingRow>
          )}

          <SettingRow label={t('settings.remoteInput.pinLabel')}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <code
                style={{
                  fontSize: 15,
                  letterSpacing: 2,
                  fontWeight: 'var(--ol-font-weight-semibold)',
                }}
              >
                {status.pin}
              </code>
              <button
                onClick={async () => {
                  try {
                    // Update local state directly from the command's returned new PIN; the
                    // backend restarts the service asynchronously, and querying status then may
                    // return running:false causing a flicker — refresh is left to the
                    // remote-input:running event.
                    const pin = await regenerateRemotePin();
                    acceptStatus((s) => (s ? { ...s, pin } : s));
                  } catch (e) {
                    console.warn('regenerateRemotePin failed', e);
                  }
                }}
                style={smallBtn}
              >
                {t('settings.remoteInput.regeneratePin')}
              </button>
            </div>
          </SettingRow>
          {viewState === 'stale' && (
            <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', marginTop: 8 }}>
              {t('settings.remoteInput.urlsStale')}
            </div>
          )}
        </>
      )}

      {enabled && viewState === 'starting' && (
        <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', marginTop: 8 }}>
          {t('settings.remoteInput.starting')}
        </div>
      )}

      {enabled && viewState === 'waiting' && (
        <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', marginTop: 8 }}>
          {t(
            'settings.remoteInput.waitingStart',
            '服务尚未启动。请关闭开关再打开一次，不要重启软件。',
          )}
        </div>
      )}

      {enabled && startError != null && (
        <div style={{ fontSize: 12, color: 'var(--ol-red, #ef4444)', marginTop: 8 }}>
          {startError.reason === 'port-in-use'
            ? t('settings.remoteInput.portInUse', { port: startError.port })
            : t('settings.remoteInput.startError', { reason: startError.reason })}
        </div>
      )}

      <div
        style={{
          fontSize: 11.5,
          color: 'var(--ol-ink-4)',
          marginTop: 10,
          lineHeight: 1.6,
        }}
      >
        {t('settings.remoteInput.securityHint')}
        <br />
        {t('settings.remoteInput.certHint')}
        <br />
        {t('settings.remoteInput.certTrustWarning')}
      </div>
    </Card>
  );
}
