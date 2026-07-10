// 高级 → Less Computer 配置：启用开关、后端（Claude / OpenCode）、模型 / 权限模式 / 工作目录。
// 「按住说话键」在 通用 → 快捷键 里配置（见 ShortcutsSection），这里不再重复。
// 配置经 UserPreferences 持久化；启用后 coordinator 才注册热键。

import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { detectOS } from '../../components/WindowChrome'
import { codingAgentDetectOpencode, type OpenCodeDetection } from '../../lib/ipc'
import type { CodingAgentPermissionMode, CodingAgentProviderId } from '../../lib/types'
import { useHotkeySettings } from '../../state/HotkeySettingsContext'
import { Card } from '../_atoms'
import { SectionDesc, SectionTitle, SettingRow, Toggle, inputStyle } from './shared'

const PERMISSION_MODES: CodingAgentPermissionMode[] = [
  'acceptEdits',
  'plan',
  'default',
  'bypassPermissions',
]

export function CodingAgentSection() {
  const { t } = useTranslation()
  const { prefs, updatePrefs: savePrefs } = useHotkeySettings()
  const os = detectOS()

  // OpenCode 安装检测：仅当启用 + 选了 OpenCode 后端时探测一次，用于提示是否需先安装。
  const [opencode, setOpencode] = useState<OpenCodeDetection | null>(null)
  const useOpencode = prefs?.codingAgentEnabled && prefs?.codingAgentProvider === 'opencode-cli'
  useEffect(() => {
    if (!useOpencode) {
      setOpencode(null)
      return
    }
    let alive = true
    // 把配置的可执行路径传给检测，用户改了路径后能重新探测对应二进制。
    void codingAgentDetectOpencode(prefs?.codingAgentExe ?? undefined).then(d => {
      if (alive) setOpencode(d)
    })
    return () => {
      alive = false
    }
  }, [useOpencode, prefs?.codingAgentExe])

  if (os === 'win') return null

  if (!prefs) {
    return (
      <Card>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
      </Card>
    )
  }

  const enabled = prefs.codingAgentEnabled

  return (
    <Card>
      <SectionTitle>{t('settings.codingAgent.title')}</SectionTitle>
      <SectionDesc>{t('settings.codingAgent.desc')}</SectionDesc>

      <SettingRow label={t('settings.codingAgent.enable')} desc={t('settings.codingAgent.hotkeyHint')}>
        <Toggle
          on={enabled}
          onToggle={next => void savePrefs({ ...prefs, codingAgentEnabled: next })}
        />
      </SettingRow>

      {enabled && (
        <>
          {/* 「按住说话键」配置已挪到 通用 → 快捷键，避免和这里重复。本区只留后端/模型等高级项。 */}
          <SettingRow label={t('settings.codingAgent.provider')}>
            <select
              value={prefs.codingAgentProvider}
              onChange={e =>
                void savePrefs({
                  ...prefs,
                  codingAgentProvider: e.target.value as CodingAgentProviderId,
                })
              }
              style={{ ...inputStyle, maxWidth: 240, cursor: 'pointer' }}
            >
              <option value="claude-code-cli">Claude Code</option>
              <option value="opencode-cli">OpenCode</option>
            </select>
          </SettingRow>

          {/* OpenCode 后端：提示安装/登录状态。issue #579。 */}
          {useOpencode && opencode && (
            <div
              style={{
                fontSize: 12,
                lineHeight: 1.6,
                color: opencode.installed ? 'var(--ol-ink-3)' : 'var(--ol-warn, #b8860b)',
                margin: '-4px 0 8px',
              }}
            >
              {opencode.installed
                ? t('settings.codingAgent.opencodeReady', { version: opencode.version ?? '?' })
                : t('settings.codingAgent.opencodeMissing')}
            </div>
          )}

          <SettingRow label={t('settings.codingConsole.permissionMode')}>
            <select
              value={prefs.codingAgentPermissionMode}
              onChange={e =>
                void savePrefs({
                  ...prefs,
                  codingAgentPermissionMode: e.target.value as CodingAgentPermissionMode,
                })
              }
              style={{ ...inputStyle, maxWidth: 240, cursor: 'pointer' }}
            >
              {PERMISSION_MODES.map(m => (
                <option key={m} value={m}>
                  {t(`settings.codingConsole.mode.${m}`)}
                </option>
              ))}
            </select>
          </SettingRow>

          <SettingRow label={t('settings.codingAgent.model')} desc={t('settings.codingAgent.modelHint')}>
            <select
              value={prefs.codingAgentModel ?? ''}
              onChange={e => {
                const v = e.target.value
                void savePrefs({ ...prefs, codingAgentModel: v === '' ? null : v })
              }}
              style={{ ...inputStyle, maxWidth: 240, cursor: 'pointer' }}
            >
              <option value="">{t('settings.codingAgent.modelDefault')}</option>
              <option value="haiku">Haiku</option>
              <option value="sonnet">Sonnet</option>
              <option value="opus">Opus</option>
            </select>
          </SettingRow>

          <SettingRow label={t('settings.codingConsole.workdir')} desc={t('settings.codingConsole.workdirDesc')}>
            <input
              type="text"
              value={prefs.codingAgentWorkdir ?? ''}
              placeholder={t('settings.codingConsole.workdirPlaceholder')}
              spellCheck={false}
              onChange={e => {
                const v = e.target.value.trim()
                void savePrefs({ ...prefs, codingAgentWorkdir: v === '' ? null : v })
              }}
              style={inputStyle}
            />
          </SettingRow>

          <SettingRow label={t('settings.codingAgent.exe')}>
            <input
              type="text"
              value={prefs.codingAgentExe ?? ''}
              placeholder={prefs.codingAgentProvider === 'opencode-cli' ? 'opencode' : 'claude'}
              spellCheck={false}
              onChange={e => {
                const v = e.target.value.trim()
                void savePrefs({ ...prefs, codingAgentExe: v === '' ? null : v })
              }}
              style={inputStyle}
            />
          </SettingRow>
        </>
      )}
    </Card>
  )
}
