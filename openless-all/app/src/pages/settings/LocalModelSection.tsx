// 高级 → 本地模型：本地 ASR 推理引擎的启用 / 禁用 + 模型下载管理。
// 自 Settings.tsx 的 AdvancedSection 拆出（流式输入已挪到「录音与输入」）。
// 含 Qwen3（macOS）/ Foundry Local + sherpa-onnx（Windows）三条本地引擎。

import { useEffect, useRef, useState } from 'react';
import type { PlatformCapabilities } from '../../lib/types';
import { useTranslation } from 'react-i18next';
import { LocalAsr } from '../LocalAsr';
import { detectOS } from '../../components/WindowChrome';
import { getPlatformCapabilities } from '../../lib/platform';
import { setActiveAsrProvider } from '../../lib/ipc';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Btn, Card } from '../_atoms';
import { SettingRow, Toggle, type AsrPresetId } from './shared';

export function LocalModelSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const os = detectOS();
  const isMac = os === 'mac';
  const isWin = os === 'win';
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  const platformSupported = platformCaps?.supportsLocalAsr === true;
  const switchSeqRef = useRef(0);
  const [busy, setBusy] = useState(false);
  // 待确认的启用目标。!== null 时中央 modal 弹出 + 背景模糊；用户点确认 → 真切；
  // 点取消 → 回到 null。一次只允许一个 modal。
  const [pendingTarget, setPendingTarget] = useState<AsrPresetId | null>(null);

  const activeAsrProvider = (prefs?.activeAsrProvider ?? 'volcengine') as AsrPresetId;
  const isOnLocalQwen3 = activeAsrProvider === 'local-qwen3';
  const isOnFoundry = activeAsrProvider === 'foundry-local-whisper';
  const isOnSherpaOnnx = activeAsrProvider === 'sherpa-onnx-local';
  const isOnAnyLocal = isOnLocalQwen3 || isOnFoundry || isOnSherpaOnnx;

  const requestEnable = (target: AsrPresetId) => {
    setPendingTarget(target);
  };

  const performSwitch = async (target: AsrPresetId) => {
    setBusy(true);
    const seq = ++switchSeqRef.current;
    try {
      await setActiveAsrProvider(target);
      if (seq !== switchSeqRef.current) return;
      if (prefs) {
        await updatePrefs({ ...prefs, activeAsrProvider: target });
      }
    } catch (err) {
      // 调用方是 void performSwitch(...) 即发即忘 —— 这里吞掉并记日志，否则 IPC
      // 失败会冒成未处理的 promise rejection。
      console.error('[settings] switch local ASR provider failed', err);
    } finally {
      if (seq === switchSeqRef.current) {
        setBusy(false);
        setPendingTarget(null);
      }
    }
  };

  const pendingNameKey =
    pendingTarget === 'local-qwen3' ? 'asrLocalQwen3'
    : pendingTarget === 'foundry-local-whisper' ? 'asrFoundryLocalWhisper'
    : pendingTarget === 'sherpa-onnx-local' ? 'asrSherpaOnnxLocal'
    : null;

  return (
    <>
      {/* ─── 屏幕中央确认 modal（背景模糊） ─────────────────────────────
          点击遮罩或取消按钮关闭；切换中（busy）禁止任何关闭路径以免半切失败。 */}
      {pendingTarget && pendingNameKey && (
        <div
          role="dialog"
          aria-modal="true"
          style={{
            position: 'fixed',
            inset: 0,
            background: 'rgba(0, 0, 0, 0.32)',
            backdropFilter: 'blur(8px)',
            WebkitBackdropFilter: 'blur(8px)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            zIndex: 1000,
            padding: 16,
          }}
          onClick={(e) => {
            if (e.target === e.currentTarget && !busy) setPendingTarget(null);
          }}>
          <Card
            style={{
              background: 'rgba(255, 188, 60, 0.12)',
              border: '1px solid rgba(220, 110, 0, 0.55)',
              maxWidth: 360,
              width: '100%',
            }}>
            <div style={{ fontSize: 13, fontWeight: 600, color: '#A04500', marginBottom: 6 }}>
              ⚠️ {t('settings.advanced.confirmEnableLocalTitle')}
            </div>
            <div style={{ fontSize: 12.5, color: 'var(--ol-ink-2)', lineHeight: 1.6, marginBottom: 10 }}>
              {t('settings.advanced.confirmEnableLocalBody', {
                target: t(`settings.providers.presets.${pendingNameKey}`),
              })}
            </div>
            <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
              <Btn variant="ghost" size="sm" disabled={busy} onClick={() => setPendingTarget(null)}>
                {t('common.cancel')}
              </Btn>
              <Btn
                variant="primary"
                size="sm"
                disabled={busy}
                onClick={() => void performSwitch(pendingTarget)}>
                {t('settings.advanced.confirm')}
              </Btn>
            </div>
          </Card>
        </div>
      )}

      <Card>
        {/* 标题 + 右上角 inline 警告小字。
            Windows：标题区整体灰显 —— 「本地 ASR 模型（实验性）」在 Win 上几乎只有
            Qwen3 占位、本平台暂不支持；Foundry / sherpa-onnx 走的是另一条独立路径。 */}
        <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 12, marginBottom: 14 }}>
          <div style={{ minWidth: 0, opacity: isWin ? 0.45 : 1 }}>
            <div style={{ fontSize: 14, fontWeight: 600, letterSpacing: '-0.01em' }}>{t('settings.advanced.localAsrTitle')}</div>
          </div>
          <div style={{
            fontSize: 11,
            color: '#A04500',
            fontWeight: 500,
            lineHeight: 1.4,
            textAlign: 'right',
            flexShrink: 0,
            maxWidth: '52%',
            paddingTop: 2,
            opacity: isWin ? 0.45 : 1,
          }}>
            ⚠️ {t('settings.advanced.localAsrWarningShort')}
          </div>
        </div>

        {!platformSupported ? (
          <div style={{ fontSize: 12.5, color: 'var(--ol-ink-3)', lineHeight: 1.6, padding: '8px 0' }}>
            {t('settings.advanced.platformNotSupported')}
          </div>
        ) : (
          <>
            {/* Qwen3 行 —— macOS Toggle 可点切换；Windows 后端是 stub，Toggle 始终 off
                + 不可点。整行灰显，跟「实验性」标题区对齐。 */}
            <div style={{ opacity: isWin ? 0.45 : 1 }}>
              <SettingRow label={t('settings.providers.presets.asrLocalQwen3')}>
                <div style={{ display: 'flex', justifyContent: 'flex-end', width: '100%' }}>
                  <Toggle
                    on={isMac && isOnLocalQwen3}
                    onToggle={isMac && !busy && pendingTarget === null ? (next) => {
                      if (next) requestEnable('local-qwen3');
                      else void performSwitch('volcengine');
                    } : undefined}
                  />
                </div>
              </SettingRow>
            </div>

            {/* Foundry Local + sherpa-onnx 行 —— 仅 Windows 露出。 */}
            {isWin && (
              <>
                <SettingRow label={t('settings.providers.presets.asrFoundryLocalWhisper')}>
                  <div style={{ display: 'flex', justifyContent: 'flex-end', width: '100%' }}>
                    <Toggle
                      on={isOnFoundry}
                      onToggle={!busy && pendingTarget === null ? (next) => {
                        if (next) requestEnable('foundry-local-whisper');
                        else void performSwitch('volcengine');
                      } : undefined}
                    />
                  </div>
                </SettingRow>
                <SettingRow label={t('settings.providers.presets.asrSherpaOnnxLocal')}>
                  <div style={{ display: 'flex', justifyContent: 'flex-end', width: '100%' }}>
                    <Toggle
                      on={isOnSherpaOnnx}
                      onToggle={!busy && pendingTarget === null ? (next) => {
                        if (next) requestEnable('sherpa-onnx-local');
                        else void performSwitch('volcengine');
                      } : undefined}
                    />
                  </div>
                </SettingRow>
              </>
            )}
          </>
        )}

        {/* 「禁用本地 ASR」逃生入口——只在行内 Toggle 关不掉的场景露出（Linux / 跨平台
            异常 profile 同步）。否则平台本机 Toggle 自身就能 off。 */}
        {isOnAnyLocal && !((isMac && isOnLocalQwen3) || (isWin && (isOnFoundry || isOnSherpaOnnx))) && (
          <SettingRow label={t('settings.advanced.disableLocalLabel')}>
            <div style={{ display: 'flex', justifyContent: 'flex-end', width: '100%' }}>
              <Btn
                variant="primary"
                size="sm"
                disabled={busy || pendingTarget !== null}
                onClick={() => void performSwitch('volcengine')}>
                {t('settings.advanced.disable')}
              </Btn>
            </div>
          </SettingRow>
        )}

        {/* 模型下载 / 加载（镜像源 · 模型列表 · 下载 · 删除 · 设为默认 · Foundry / sherpa）
            —— 跟上面的启动开关收进同一个框：「本地 ASR」是一个整体。 */}
        {platformSupported && (
          <div style={{ marginTop: 16, borderTop: '0.5px solid var(--ol-line)', paddingTop: 16 }}>
            <LocalAsr embedded />
          </div>
        )}
      </Card>
    </>
  );
}
