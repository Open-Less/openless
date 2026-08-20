// 高级 → Less Computer 配置：启用开关、后端（Claude / OpenCode / Codex）、
// 模型 / 权限模式 / 工作目录。
//
// 四个后端的能力不一样，这一页要如实反映差异，别让用户以为选项都通用：
// - 模型：Claude 用别名下拉，OpenCode 拉账号可用列表，Codex 收裸模型名（自由文本），
// - 护栏：Claude / OpenCode 是逐命令 deny 清单（撞了能弹审批卡放行单条）；
//   Codex 只有粗粒度沙箱档位，审批卡对它不生效，这里挂一条说明。
// 「按住说话键」在 通用 → 快捷键 里配置（见 ShortcutsSection），这里不再重复。
// 配置经 UserPreferences 持久化；启用后 coordinator 才注册热键。

import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { detectOS } from '../../components/WindowChrome'
import {
  codingAgentDetectCli,
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
const CODEX_PERMISSION_MODES: CodingAgentPermissionMode[] = ['plan', 'acceptEdits']

function permissionModesForProvider(provider: CodingAgentProviderId) {
  return provider === 'codex-cli' ? CODEX_PERMISSION_MODES : PERMISSION_MODES
}

function normalizePermissionMode(
  provider: CodingAgentProviderId,
  mode: CodingAgentPermissionMode,
): CodingAgentPermissionMode {
  return provider === 'codex-cli' && (mode === 'default' || mode === 'bypassPermissions') ? 'plan' : mode
}

type OpenCodeModelsStatus = 'idle' | 'loading' | 'loaded' | 'error'

/** 后端下拉的选项。顺序 = 接入先后，Claude 保持第一（默认后端）。 */
const PROVIDERS: { value: CodingAgentProviderId; label: string }[] = [
  { value: 'claude-code-cli', label: 'Claude Code' },
  { value: 'opencode-cli', label: 'OpenCode' },
  { value: 'codex-cli', label: 'Codex' },
]

/** 各后端默认的可执行文件名，用作「自定义路径」输入框的 placeholder。 */
const DEFAULT_EXE: Record<CodingAgentProviderId, string> = {
  'claude-code-cli': 'claude',
  'opencode-cli': 'opencode',
  'codex-cli': 'codex',
}

export function CodingAgentSection() {
  const { t } = useTranslation()
  const { prefs, updatePrefs: savePrefs } = useHotkeySettings()
  const os = detectOS()

  // OpenCode 安装检测：仅当启用 + 选了 OpenCode 后端时探测一次，用于提示是否需先安装。
  const [opencode, setOpencode] = useState<OpenCodeDetection | null>(null)
  const [opencodeModels, setOpencodeModels] = useState<string[]>([])
  const [opencodeModelsStatus, setOpencodeModelsStatus] = useState<OpenCodeModelsStatus>('idle')
  const [opencodeModelsError, setOpencodeModelsError] = useState('')

  const provider: CodingAgentProviderId = prefs?.codingAgentProvider ?? 'claude-code-cli'
  const useOpencode = prefs?.codingAgentEnabled && provider === 'opencode-cli'
  const useCodex = prefs?.codingAgentEnabled && provider === 'codex-cli'
  // 只有沙箱档位、没有逐命令 deny 清单的后端：审批卡对它们不生效。
  const sandboxOnly = Boolean(useCodex)

  // Codex 的安装检测（走通用检测命令，后续接别的后端时可以复用）。
  const [cliDetection, setCliDetection] = useState<OpenCodeDetection | null>(null)
  useEffect(() => {
    if (!sandboxOnly) {
      setCliDetection(null)
      return
    }
    let alive = true
    setCliDetection(null)
    void (async () => {
      try {
        const detection = await codingAgentDetectCli(provider, prefs?.codingAgentExe ?? undefined)
        if (alive) setCliDetection(detection)
      } catch {
        // 检测失败按「没装」处理：这里只是提示，不阻断用户保存配置。
        if (alive) setCliDetection({ installed: false, version: null, exe: DEFAULT_EXE[provider] })
      }
    })()
    return () => {
      alive = false
    }
  }, [sandboxOnly, provider, prefs?.codingAgentExe])
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

  useEffect(() => {
    if (
      !prefs ||
      provider !== 'codex-cli' ||
      (prefs.codingAgentPermissionMode !== 'default' && prefs.codingAgentPermissionMode !== 'bypassPermissions')
    ) {
      return
    }
    void savePrefs({ ...prefs, codingAgentPermissionMode: 'plan' })
  }, [prefs, provider, savePrefs])

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
              onChange={v => {
                const nextProvider = v as CodingAgentProviderId
                void savePrefs({
                  ...prefs,
                  codingAgentProvider: nextProvider,
                  codingAgentModel: null,
                  codingAgentPermissionMode: normalizePermissionMode(
                    nextProvider,
                    prefs.codingAgentPermissionMode,
                  ),
                })
              }}
              options={PROVIDERS}
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

          {/* Codex：装没装 + 版本。没装时按警示色提示。 */}
          {sandboxOnly && cliDetection && (
            <div
              style={{
                fontSize: 12,
                lineHeight: 1.6,
                color: cliDetection.installed ? 'var(--ol-ink-3)' : 'var(--ol-warn, #b8860b)',
                margin: '-4px 0 8px',
              }}
            >
              {cliDetection.installed
                ? t('settings.codingAgent.cliReady', {
                    name: DEFAULT_EXE[provider],
                    version: cliDetection.version ?? '?',
                  })
                : t('settings.codingAgent.cliMissing', { name: DEFAULT_EXE[provider] })}
            </div>
          )}

          {/* 护栏差异说明：这两家没有逐命令 deny 清单，审批卡不会弹。别让用户以为有。 */}
          {sandboxOnly && (
            <div
              style={{
                fontSize: 12,
                lineHeight: 1.6,
                color: 'var(--ol-ink-4)',
                margin: '-4px 0 8px',
              }}
            >
              {t('settings.codingAgent.sandboxGuardHint')}
            </div>
          )}

          {useCodex && (
            <div
              style={{
                fontSize: 12,
                lineHeight: 1.6,
                color: 'var(--ol-ink-4)',
                margin: '-4px 0 8px',
              }}
            >
              {t('settings.codingAgent.codexBudgetHint')}
            </div>
          )}

          <SettingRow label={t('settings.codingConsole.permissionMode')}>
            <SelectLite
              value={normalizePermissionMode(provider, prefs.codingAgentPermissionMode)}
              onChange={v => void savePrefs({ ...prefs, codingAgentPermissionMode: v as CodingAgentPermissionMode })}
              options={permissionModesForProvider(provider).map(m => ({
                value: m,
                label: t(
                  provider === 'codex-cli'
                    ? `settings.codingAgent.codexMode.${m === 'acceptEdits' ? 'workspaceWrite' : 'plan'}`
                    : `settings.codingConsole.mode.${m}`,
                ),
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
                : useCodex
                  ? 'settings.codingAgent.codexModelHint'
                  : 'settings.codingAgent.modelHint',
            )}
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
              {useCodex ? (
                // Codex 的模型名是裸名（gpt-5 / o3 / 自建网关的任意名字），枚举不过来，
                // 给自由文本；留空 = 用 ~/.codex/config.toml 里的设置。
                <input
                  type="text"
                  value={prefs.codingAgentModel ?? ''}
                  placeholder={t('settings.codingAgent.codexModelPlaceholder')}
                  spellCheck={false}
                  aria-label={t('settings.codingAgent.model')}
                  onChange={e => {
                    const v = e.target.value.trim()
                    void savePrefs({ ...prefs, codingAgentModel: v === '' ? null : v })
                  }}
                  style={{ ...inputStyle, maxWidth: 300 }}
                />
              ) : (
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
              )}
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
              placeholder={DEFAULT_EXE[provider]}
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
