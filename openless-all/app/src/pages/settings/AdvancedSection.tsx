// 高级设置：流式输入开关 / 同步剪贴板 / 本地 ASR 模型启用与禁用。
// 拆出自 Settings.tsx，逻辑零改动；i18n key 全部保持 `settings.advanced.*`。

import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { LocalAsr } from '../LocalAsr';
import { detectOS } from '../../components/WindowChrome';
import { setActiveAsrProvider } from '../../lib/ipc';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Btn, Card } from '../_atoms';
import { SettingRow, Toggle, type AsrPresetId } from './shared';

export function AdvancedSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const os = detectOS();
  const isMac = os === 'mac';
  const isWin = os === 'win';
  const isLinux = os === 'linux';
  const platformSupported = isMac || isWin;
  const switchSeqRef = useRef(0);
  const [busy, setBusy] = useState(false);
  // 待确认的启用目标。!== null 时中央 modal 弹出 + 背景模糊；用户点确认 → 真切；
  // 点取消 → 回到 null。一次只允许一个 modal。
  const [pendingTarget, setPendingTarget] = useState<AsrPresetId | null>(null);

  const activeAsrProvider = (prefs?.activeAsrProvider ?? 'volcengine') as AsrPresetId;
  const isOnLocalQwen3 = activeAsrProvider === 'local-qwen3';
  const isOnFoundry = activeAsrProvider === 'foundry-local-whisper';
  const isOnAnyLocal = isOnLocalQwen3 || isOnFoundry;

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

      {/* ─── 流式输入（全平台 opt-in） ───────────────────────────────────
          润色 SSE 一边到达一边逐字模拟键盘事件落到光标。开启后用户感知到的处理
          时延显著降低，但有几个限制（不满足时自动回落原一次性插入路径）：
          - macOS：CGEvent Unicode + 临时切到 ABC 输入源（CJK / 日文 IME 拦截兜底）
          - Windows：SendInput Unicode，绕过 TSF / IME，不需要切输入法
          - Linux（实验）：enigo XTest；Wayland compositor 拒绝 libei 时失败回落
          - 仅 OpenAI-compatible provider 实装；Gemini / Codex 透明降级
          - 密码框 / 1Password / SSH prompt 等 Secure Input 框拒绝合成按键 → 失败回落
          每个平台用各自的 hint key，互相不显示对方平台的细节。 */}
      <Card>
        <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>
          {t(isLinux
            ? 'settings.advanced.streamingInsertTitleLinux'
            : 'settings.advanced.streamingInsertTitle')}
        </div>
        <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginBottom: 10 }}>
          {t('settings.advanced.streamingInsertDesc')}
        </div>
        <SettingRow
          label={t('settings.advanced.streamingInsertLabel')}
          desc={t(
            isMac
              ? 'settings.advanced.streamingInsertHintMac'
              : isWin
                ? 'settings.advanced.streamingInsertHintWindows'
                : 'settings.advanced.streamingInsertHintLinux'
          )}>
          <Toggle
            on={!!prefs?.streamingInsert}
            onToggle={(next) => {
              if (prefs) void updatePrefs({ ...prefs, streamingInsert: next });
            }}
          />
        </SettingRow>
        <SettingRow
          label={t('settings.advanced.streamingInsertSaveClipboardLabel')}
          desc={t('settings.advanced.streamingInsertSaveClipboardHint')}>
          <Toggle
            on={!!prefs?.streamingInsertSaveClipboard}
            onToggle={(next) => {
              if (prefs) void updatePrefs({ ...prefs, streamingInsertSaveClipboard: next });
            }}
          />
        </SettingRow>
      </Card>

      <Card>
        {/* 标题 + 右上角 inline 警告小字（替换原琥珀大警告条）。 */}
        <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 12, marginBottom: 14 }}>
          <div style={{ minWidth: 0 }}>
            <div style={{ fontSize: 13, fontWeight: 600 }}>{t('settings.advanced.localAsrTitle')}</div>
            <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 2 }}>
              {t('settings.advanced.localAsrDesc')}
            </div>
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
                + 不可点 + desc=notSupportedHere，跟"本平台不可用"视觉一致。跨平台
                异常（Windows profile 同步到 local-qwen3）时 active 状态靠下方独立
                "禁用本地 ASR" 行兜底，避免 Toggle ON + desc 说不支持的自相矛盾感
                （pr_agent #403 'Stale Windows state' 修法）。 */}
            <SettingRow
              label={t('settings.providers.presets.asrLocalQwen3')}
              desc={isMac ? t('settings.advanced.qwen3Desc') : t('settings.advanced.notSupportedHere')}>
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

            {/* Foundry 行 —— 仅 Windows 露出（macOS 不展示 Windows 端模型内容）。 */}
            {isWin && (
              <SettingRow
                label={t('settings.providers.presets.asrFoundryLocalWhisper')}
                desc={t('settings.advanced.foundryDesc')}>
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
            )}
          </>
        )}

        {/* 「禁用本地 ASR」逃生入口——只在行内 Toggle 关不掉的场景露出：
            - Linux / 不支持平台：根本没有任何引擎行
            - 跨平台异常（macOS profile 同步到 foundry / Windows profile 同步到 qwen3）：
              本机引擎 Toggle 是 off，关不动异常 active 的对方引擎
            否则平台本机 Toggle 自身就能 off → 关停，重复 disable 行徒增视觉。 */}
        {isOnAnyLocal && !((isMac && isOnLocalQwen3) || (isWin && isOnFoundry)) && (
          <SettingRow
            label={t('settings.advanced.disableLocalLabel')}
            desc={t('settings.advanced.disableLocalDesc')}>
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
      </Card>

      {/* 模型管理 UI（镜像源 / 模型列表 / 下载 / 删除 / 设为默认 / Foundry Local）
          inline 渲染——「模型设置」独立页已删，这里是唯一入口。 */}
      {platformSupported && <LocalAsr embedded />}
    </>
  );
}
