// 高级 → Less Computer 配置：启用开关、后端（Claude / OpenCode）、模型 / 权限模式 / 工作目录。
// 「按住说话键」在 通用 → 快捷键 里配置（见 ShortcutsSection），这里不再重复。
// 配置经 UserPreferences 持久化；启用后 coordinator 才注册热键。

import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { detectOS } from '../../components/WindowChrome'
import {
  codingAgentDetectOpencode,
  codingAgentListOpencodeModels,
  lessComputerWindowOpen,
  type OpenCodeDetection,
} from '../../lib/ipc'
import type { CodingAgentPermissionMode, CodingAgentProviderId } from '../../lib/types'
import { useHotkeySettings } from '../../state/HotkeySettingsContext'
import { SelectLite } from '../../components/ui/SelectLite'
import { Card } from '../_atoms'
import { SectionDesc, SectionTitle, SettingRow, Toggle, inputStyle } from './shared'

const PERMISSION_MODES: CodingAgentPermissionMode[] = [
  'acceptEdits',
  'plan',
  'default',
  'bypassPermissions',
]

type OpenCodeModelsStatus = 'idle' | 'loading' | 'loaded' | 'error'

export function CodingAgentSection() {
  const { t } = useTranslation()
  const { prefs, updatePrefs: savePrefs } = useHotkeySettings()
  const os = detectOS()

  // OpenCode 安装检测：仅当启用 + 选了 OpenCode 后端时探测一次，用于提示是否需先安装。
  const [opencode, setOpencode] = useState<OpenCodeDetection | null>(null)
  const [opencodeModels, setOpencodeModels] = useState<string[]>([])
  const [opencodeModelsStatus, setOpencodeModelsStatus] = useState<OpenCodeModelsStatus>('idle')
  const [opencodeModelsError, setOpencodeModelsError] = useState('')
  const useOpencode = prefs?.codingAgentEnabled && prefs?.codingAgentProvider === 'opencode-cli'
  useEffect(() => {
    if (!useOpencode) {
      setOpencode(null)
      setOpencodeModels([])
      setOpencodeModelsStatus('idle')
      setOpencodeModelsError('')
      return
    }
    let alive = true
    setOpencode(null)
    setOpencodeModels([])
    setOpencodeModelsStatus('loading')
    setOpencodeModelsError('')
    // 先探测用户配置的二进制，再自动刷新当前 OpenCode 账号可用的模型。
    void (async () => {
      try {
        const exe = prefs?.codingAgentExe ?? undefined
        const detection = await codingAgentDetectOpencode(exe)
        if (!alive) return
        setOpencode(detection)
        if (!detection.installed) {
          setOpencodeModelsStatus('idle')
          return
        }
        const models = await codingAgentListOpencodeModels(exe, true)
        if (!alive) return
        setOpencodeModels(models)
        setOpencodeModelsStatus('loaded')
      } catch (error) {
        if (!alive) return
        setOpencodeModelsError(error instanceof Error ? error.message : String(error))
        setOpencodeModelsStatus('error')
      }
    })()
    return () => {
      alive = false
    }
  }, [useOpencode, prefs?.codingAgentExe])

  const refreshOpencodeModels = async () => {
    setOpencodeModelsStatus('loading')
    setOpencodeModelsError('')
    try {
      const models = await codingAgentListOpencodeModels(prefs?.codingAgentExe ?? undefined, true)
      setOpencodeModels(models)
      setOpencodeModelsStatus('loaded')
    } catch (error) {
      setOpencodeModelsError(error instanceof Error ? error.message : String(error))
      setOpencodeModelsStatus('error')
    }
  }

  // Less Computer 仅 macOS 开放：后端只在 macOS 注册热键/创建窗口，
  // Windows / Linux 不渲染配置入口，避免用户看到无法使用的功能。
  if (os === 'win' || os === 'linux') return null

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
      <SectionTitle hint={t('settings.codingAgent.desc')}>{t('settings.codingAgent.title')}</SectionTitle>
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
            <SelectLite
              value={prefs.codingAgentProvider}
              onChange={v =>
                void savePrefs({
                  ...prefs,
                  codingAgentProvider: v as CodingAgentProviderId,
                  codingAgentModel: null,
                })
              }
              options={[
                { value: 'claude-code-cli', label: 'Claude Code' },
                { value: 'opencode-cli', label: 'OpenCode' },
              ]}
              ariaLabel={t('settings.codingAgent.provider')}
              style={{ ...inputStyle, maxWidth: 240 }}
            />
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
            <SelectLite
              value={prefs.codingAgentPermissionMode}
              onChange={v => void savePrefs({ ...prefs, codingAgentPermissionMode: v as CodingAgentPermissionMode })}
              options={PERMISSION_MODES.map(m => ({
                value: m,
                label: t(`settings.codingConsole.mode.${m}`),
              }))}
              ariaLabel={t('settings.codingConsole.permissionMode')}
              style={{ ...inputStyle, maxWidth: 240 }}
            />
          </SettingRow>

          <SettingRow
            label={t('settings.codingAgent.model')}
            desc={t(
              useOpencode
                ? 'settings.codingAgent.opencodeModelHint'
                : 'settings.codingAgent.modelHint',
            )}
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
              <SelectLite
                value={
                  useOpencode
                    ? prefs.codingAgentModel?.includes('/')
                      ? prefs.codingAgentModel
                      : ''
                    : (prefs.codingAgentModel ?? '')
                }
                onChange={v => void savePrefs({ ...prefs, codingAgentModel: v === '' ? null : v })}
                options={
                  useOpencode
                    ? [
                        // 空值 = 使用 OpenCode CLI 默认模型。
                        { value: '', label: t('settings.codingAgent.opencodeModelDefault') },
                        // 已选但不在拉取结果里的模型仍保留，避免选中项凭空消失。
                        ...(prefs.codingAgentModel?.includes('/') &&
                        !opencodeModels.includes(prefs.codingAgentModel)
                          ? [{ value: prefs.codingAgentModel, label: prefs.codingAgentModel }]
                          : []),
                        ...opencodeModels.map(model => ({ value: model, label: model })),
                      ]
                    : [
                        // 空值 = 使用 CLI 默认模型；放回选项里，避免选了具体模型后回不去默认。
                        { value: '', label: t('settings.codingAgent.modelDefault') },
                        { value: 'haiku', label: 'Haiku' },
                        { value: 'sonnet', label: 'Sonnet' },
                        { value: 'opus', label: 'Opus' },
                      ]
                }
                ariaLabel={t('settings.codingAgent.model')}
                style={{ ...inputStyle, maxWidth: 300 }}
              />
              {useOpencode && opencode?.installed && (
                <button
                  type="button"
                  disabled={opencodeModelsStatus === 'loading'}
                  onClick={() => void refreshOpencodeModels()}
                  style={{
                    ...inputStyle,
                    width: 'auto',
                    cursor: opencodeModelsStatus === 'loading' ? 'default' : 'pointer',
                    opacity: opencodeModelsStatus === 'loading' ? 0.65 : 1,
                  }}
                >
                  {t(
                    opencodeModelsStatus === 'loading'
                      ? 'settings.codingAgent.opencodeModelsRefreshing'
                      : 'settings.codingAgent.opencodeModelsRefresh',
                  )}
                </button>
              )}
            </div>
          </SettingRow>

          {useOpencode && opencode?.installed && opencodeModelsStatus !== 'idle' && (
            <div
              role={opencodeModelsStatus === 'error' ? 'alert' : 'status'}
              style={{
                fontSize: 12,
                lineHeight: 1.6,
                color:
                  opencodeModelsStatus === 'error'
                    ? 'var(--ol-warn, #b8860b)'
                    : 'var(--ol-ink-4)',
                margin: '-4px 0 8px',
              }}
            >
              {opencodeModelsStatus === 'loading'
                ? t('settings.codingAgent.opencodeModelsRefreshing')
                : opencodeModelsStatus === 'error'
                  ? t('settings.codingAgent.opencodeModelsError', {
                      message: opencodeModelsError,
                    })
                  : opencodeModels.length > 0
                    ? t('settings.codingAgent.opencodeModelsLoaded', {
                        count: opencodeModels.length,
                      })
                    : t('settings.codingAgent.opencodeModelsEmpty')}
            </div>
          )}

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

          <SettingRow
            label={t('settings.codingAgent.openPanel')}
            desc={t('settings.codingAgent.openPanelHint')}
          >
            <button
              type="button"
              onClick={() => void lessComputerWindowOpen()}
              style={{ ...inputStyle, width: 'auto', cursor: 'pointer' }}
            >
              {t('settings.codingAgent.openPanelAction')}
            </button>
          </SettingRow>
        </>
      )}
    </Card>
  )
}
