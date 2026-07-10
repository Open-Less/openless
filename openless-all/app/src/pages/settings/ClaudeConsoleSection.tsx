// 高级 → Claude 控制台：检测 claude 安装 / MCP（computer use）状态，
// 并护栏化地无头跑一次 claude、流式查看输出与用量。这是「快速 Agent」引擎的
// 最小可用垂直切片，不依赖录音 / coordinator。

import { useEffect, useRef, useState, type CSSProperties } from 'react'
import { useTranslation } from 'react-i18next'
import {
  codingAgentCancelTest,
  codingAgentCommandRisk,
  codingAgentDetect,
  codingAgentRunTest,
  isTauri,
  type ClaudeDetection,
  type CodingAgentEvent,
  type CodingAgentPermissionMode,
} from '../../lib/ipc'
import { Btn, Card } from '../_atoms'
import { SectionDesc, SectionTitle, SettingRow, inputStyle } from './shared'

const PERMISSION_MODES: CodingAgentPermissionMode[] = [
  'acceptEdits',
  'plan',
  'default',
  'bypassPermissions',
]

const consoleStyle: CSSProperties = {
  margin: 0,
  marginTop: 10,
  padding: '12px 14px',
  minHeight: 96,
  maxHeight: 260,
  overflow: 'auto',
  borderRadius: 10,
  background: '#0f1117',
  color: '#d7dbe0',
  fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
  fontSize: 12,
  lineHeight: 1.6,
  whiteSpace: 'pre-wrap',
  wordBreak: 'break-word',
}

export function ClaudeConsoleSection() {
  const { t } = useTranslation()
  const [detection, setDetection] = useState<ClaudeDetection | null>(null)
  const [detecting, setDetecting] = useState(false)
  const [exe, setExe] = useState('claude')
  const [workdir, setWorkdir] = useState('')
  const [permMode, setPermMode] = useState<CodingAgentPermissionMode>('acceptEdits')
  const [prompt, setPrompt] = useState('')
  const [running, setRunning] = useState(false)
  const [output, setOutput] = useState('')
  const [summary, setSummary] = useState<string | null>(null)
  const [risk, setRisk] = useState<string | null>(null)
  // 控制台默认折叠：测试用的重型 UI（检测/输入/输出）平时不展开，保持设置页清爽。
  const [expanded, setExpanded] = useState(false)
  const outRef = useRef<HTMLPreElement | null>(null)

  async function runDetect() {
    setDetecting(true)
    try {
      setDetection(await codingAgentDetect(exe.trim() || undefined))
    } finally {
      setDetecting(false)
    }
  }

  // 订阅后端流式事件。
  useEffect(() => {
    if (!isTauri) return
    let unlisten: (() => void) | undefined
    let alive = true
    void (async () => {
      const { listen } = await import('@tauri-apps/api/event')
      const un = await listen<CodingAgentEvent>('coding-agent:test', e => {
        const ev = e.payload
        switch (ev.kind) {
          case 'started':
            setOutput('')
            setSummary(null)
            break
          case 'delta':
            setOutput(prev => prev + ev.text)
            break
          case 'tool_use':
            setOutput(prev => `${prev}\n· ${t('settings.codingConsole.toolUse', { name: ev.name })}\n`)
            break
          case 'completed':
            setRunning(false)
            setSummary(
              ev.cost_usd != null
                ? t('settings.codingConsole.doneCost', { cost: ev.cost_usd.toFixed(4) })
                : t('settings.codingConsole.done'),
            )
            if (ev.text.trim()) setOutput(prev => (prev.trim() ? prev : ev.text))
            break
          case 'cancelled':
            setRunning(false)
            setSummary(t('settings.codingConsole.cancelled'))
            break
          case 'error':
            setRunning(false)
            setSummary(`✗ ${ev.message}`)
            break
        }
      })
      if (alive) unlisten = un
      else un()
    })()
    return () => {
      alive = false
      unlisten?.()
    }
  }, [t])

  // 首次自动检测。
  useEffect(() => {
    void runDetect()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // 输出自动滚到底。
  useEffect(() => {
    if (outRef.current) outRef.current.scrollTop = outRef.current.scrollHeight
  }, [output])

  const onRun = async () => {
    const p = prompt.trim()
    if (!p || running) return
    setRisk(await codingAgentCommandRisk(p))
    setRunning(true)
    setOutput('')
    setSummary(null)
    try {
      await codingAgentRunTest({
        prompt: p,
        exe: exe.trim() || undefined,
        permissionMode: permMode,
        workdir: workdir.trim() || undefined,
        maxBudgetUsd: 0.5,
      })
    } catch (err) {
      setRunning(false)
      setSummary(`✗ ${err instanceof Error ? err.message : String(err)}`)
    }
  }

  const installed = detection?.installed === true

  return (
    <Card>
      <button
        onClick={() => setExpanded(v => !v)}
        style={{ all: 'unset', cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 8, width: '100%' }}
      >
        <SectionTitle>{t('settings.codingConsole.title')}</SectionTitle>
        <span style={{ marginLeft: 'auto', fontSize: 12, color: 'var(--ol-ink-4)' }}>
          {expanded ? `${t('common.hide')} ▴` : `${t('common.show')} ▾`}
        </span>
      </button>
      <SectionDesc>{t('settings.codingConsole.desc')}</SectionDesc>

      {expanded && (
        <>
      <SettingRow label={t('settings.codingConsole.status')} desc={t('settings.codingConsole.guardNote')}>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6, width: '100%' }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
            <Btn variant="ghost" size="sm" disabled={detecting} onClick={() => void runDetect()}>
              {detecting ? t('settings.codingConsole.detecting') : t('settings.codingConsole.detect')}
            </Btn>
            {detection && (
              <span style={{ fontSize: 12, color: installed ? 'var(--ol-ok)' : 'var(--ol-err)' }}>
                {installed
                  ? `${t('settings.codingConsole.installed')} · v${detection.version ?? '?'}`
                  : t('settings.codingConsole.notInstalled')}
              </span>
            )}
          </div>
          {detection && !installed && (
            <span style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.5 }}>
              {t('settings.codingConsole.notInstalledHint')}
            </span>
          )}
          {detection && installed && (
            <span style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.5 }}>
              {t('settings.codingConsole.mcpServers', { count: detection.mcpServers.length })}
              {' · '}
              {detection.hasComputerUse
                ? t('settings.codingConsole.computerUsePresent')
                : t('settings.codingConsole.computerUseAbsent')}
            </span>
          )}
        </div>
      </SettingRow>

      <SettingRow label={t('settings.codingConsole.exePath')}>
        <input
          type="text"
          value={exe}
          placeholder="claude"
          spellCheck={false}
          onChange={e => setExe(e.target.value)}
          style={inputStyle}
        />
      </SettingRow>

      <SettingRow label={t('settings.codingConsole.workdir')} desc={t('settings.codingConsole.workdirDesc')}>
        <input
          type="text"
          value={workdir}
          placeholder={t('settings.codingConsole.workdirPlaceholder')}
          spellCheck={false}
          onChange={e => setWorkdir(e.target.value)}
          style={inputStyle}
        />
      </SettingRow>

      <SettingRow label={t('settings.codingConsole.permissionMode')}>
        <select
          value={permMode}
          onChange={e => setPermMode(e.target.value as CodingAgentPermissionMode)}
          style={{ ...inputStyle, maxWidth: 220, cursor: 'pointer' }}
        >
          {PERMISSION_MODES.map(m => (
            <option key={m} value={m}>
              {t(`settings.codingConsole.mode.${m}`)}
            </option>
          ))}
        </select>
      </SettingRow>

      <div style={{ paddingTop: 14, borderTop: '0.5px solid var(--ol-line-soft)' }}>
        <textarea
          value={prompt}
          placeholder={t('settings.codingConsole.promptPlaceholder')}
          rows={3}
          spellCheck={false}
          onChange={e => setPrompt(e.target.value)}
          style={{
            ...inputStyle,
            maxWidth: '100%',
            height: 'auto',
            padding: '10px 12px',
            resize: 'vertical',
            lineHeight: 1.5,
          }}
        />
        {risk && (
          <div style={{ marginTop: 8, fontSize: 11.5, color: 'var(--ol-err)', lineHeight: 1.5 }}>
            ⚠ {t('settings.codingConsole.riskWarn', { reason: risk })}
          </div>
        )}
        <div style={{ display: 'flex', gap: 8, marginTop: 10, alignItems: 'center' }}>
          <Btn
            variant="primary"
            size="sm"
            disabled={!installed || running || prompt.trim() === ''}
            onClick={() => void onRun()}
          >
            {running ? t('settings.codingConsole.running') : t('settings.codingConsole.run')}
          </Btn>
          {running && (
            <Btn variant="ghost" size="sm" onClick={() => void codingAgentCancelTest()}>
              {t('settings.codingConsole.cancel')}
            </Btn>
          )}
          {!running && output !== '' && (
            <Btn
              variant="ghost"
              size="sm"
              onClick={() => {
                setOutput('')
                setSummary(null)
              }}
            >
              {t('settings.codingConsole.clear')}
            </Btn>
          )}
          {summary && (
            <span style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginLeft: 'auto' }}>{summary}</span>
          )}
        </div>
        <pre ref={outRef} style={consoleStyle}>
          {output || t('settings.codingConsole.outputPlaceholder')}
        </pre>
      </div>
        </>
      )}
    </Card>
  )
}
