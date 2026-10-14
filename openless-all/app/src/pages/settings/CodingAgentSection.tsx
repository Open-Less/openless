// Advanced → Less Computer configuration: enable toggle, backend
// (Claude / OpenCode / Codex / dsh), model / permission mode / working directory.
//
// The four backends differ in capability, and this page must reflect that honestly so users
// don't assume every option applies everywhere:
// - Model: Claude uses an alias dropdown, OpenCode fetches the account's available list, Codex
//   takes a bare model name (free text), and dsh has no model switch at all — hide that row.
// - Guardrails: Claude / OpenCode have per-command deny lists (a hit can raise an approval card
//   to allow that one command); Codex / dsh only have coarse sandbox levels where the approval
//   card has no effect, so show an explanatory note here.
// "Hold-to-talk key" is configured under General → Shortcuts (see ShortcutsSection); not repeated here.
// Configuration persists via UserPreferences; the coordinator registers the hotkey only when enabled.

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  codingAgentDetectCli,
  codingAgentDetectOpencode,
  codingAgentListOpencodeModels,
  lessComputerWindowOpen,
  type OpenCodeDetection,
} from '../../lib/ipc';
import type { CodingAgentPermissionMode, CodingAgentProviderId } from '../../lib/types';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { SelectLite } from '../../components/ui/SelectLite';
import { Card } from '../_atoms';
import { SettingRow, Toggle, inputStyle } from './shared';

const PERMISSION_MODES: CodingAgentPermissionMode[] = [
  'acceptEdits',
  'plan',
  'default',
  'bypassPermissions',
];
const SANDBOX_PERMISSION_MODES: CodingAgentPermissionMode[] = ['plan', 'acceptEdits'];

function isSandboxPermissionProvider(provider: CodingAgentProviderId) {
  return provider === 'codex-cli' || provider === 'dsh-cli';
}

function permissionModesForProvider(provider: CodingAgentProviderId) {
  return isSandboxPermissionProvider(provider) ? SANDBOX_PERMISSION_MODES : PERMISSION_MODES;
}

function normalizePermissionMode(
  provider: CodingAgentProviderId,
  mode: CodingAgentPermissionMode,
): CodingAgentPermissionMode {
  return isSandboxPermissionProvider(provider) &&
    (mode === 'default' || mode === 'bypassPermissions')
    ? 'plan'
    : mode;
}

type OpenCodeModelsStatus = 'idle' | 'loading' | 'loaded' | 'error';

/** Backend dropdown options. Order = onboarding order; Claude stays first (default backend). */
const PROVIDERS: { value: CodingAgentProviderId; label: string }[] = [
  { value: 'claude-code-cli', label: 'Claude Code' },
  { value: 'opencode-cli', label: 'OpenCode' },
  { value: 'codex-cli', label: 'Codex' },
  { value: 'dsh-cli', label: 'dsh' },
];

/** Default executable name per backend, used as the "custom path" input placeholder. */
const DEFAULT_EXE: Record<CodingAgentProviderId, string> = {
  'claude-code-cli': 'claude',
  'opencode-cli': 'opencode',
  'codex-cli': 'codex',
  'dsh-cli': 'dsh',
};

export function CodingAgentSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs: savePrefs } = useHotkeySettings();

  // OpenCode install detection: probe once only when enabled + the OpenCode backend is selected,
  // to hint whether it must be installed first.
  const [opencode, setOpencode] = useState<OpenCodeDetection | null>(null);
  const [opencodeModels, setOpencodeModels] = useState<string[]>([]);
  const [opencodeModelsStatus, setOpencodeModelsStatus] = useState<OpenCodeModelsStatus>('idle');
  const [opencodeModelsError, setOpencodeModelsError] = useState('');
  const [openPanelBusy, setOpenPanelBusy] = useState(false);
  const [openPanelError, setOpenPanelError] = useState('');

  const openPanel = async () => {
    if (openPanelBusy) return;
    setOpenPanelBusy(true);
    setOpenPanelError('');
    try {
      await lessComputerWindowOpen();
    } catch (error) {
      setOpenPanelError(error instanceof Error ? error.message : String(error));
    } finally {
      setOpenPanelBusy(false);
    }
  };

  const provider: CodingAgentProviderId = prefs?.codingAgentProvider ?? 'claude-code-cli';
  const useOpencode = prefs?.codingAgentEnabled && provider === 'opencode-cli';
  const useCodex = prefs?.codingAgentEnabled && provider === 'codex-cli';
  const useDsh = prefs?.codingAgentEnabled && provider === 'dsh-cli';
  // Backends with only sandbox levels and no per-command deny list: the approval card has no effect on them.
  const sandboxOnly = Boolean(useCodex || useDsh);

  // Codex / dsh install detection (both share the same generic detection command).
  const [cliDetection, setCliDetection] = useState<OpenCodeDetection | null>(null);
  useEffect(() => {
    if (!sandboxOnly) {
      setCliDetection(null);
      return;
    }
    let alive = true;
    setCliDetection(null);
    void (async () => {
      try {
        const detection = await codingAgentDetectCli(provider, prefs?.codingAgentExe ?? undefined);
        if (alive) setCliDetection(detection);
      } catch {
        // Treat detection failure as "not installed": this is only a hint, it never blocks saving the config.
        if (alive) setCliDetection({ installed: false, version: null, exe: DEFAULT_EXE[provider] });
      }
    })();
    return () => {
      alive = false;
    };
  }, [sandboxOnly, provider, prefs?.codingAgentExe]);
  useEffect(() => {
    if (!useOpencode) {
      setOpencode(null);
      setOpencodeModels([]);
      setOpencodeModelsStatus('idle');
      setOpencodeModelsError('');
      return;
    }
    let alive = true;
    setOpencode(null);
    setOpencodeModels([]);
    setOpencodeModelsStatus('loading');
    setOpencodeModelsError('');
    // Probe the user-configured binary first, then auto-refresh the models available to the current OpenCode account.
    void (async () => {
      try {
        const exe = prefs?.codingAgentExe ?? undefined;
        const detection = await codingAgentDetectOpencode(exe);
        if (!alive) return;
        setOpencode(detection);
        if (!detection.installed) {
          setOpencodeModelsStatus('idle');
          return;
        }
        const models = await codingAgentListOpencodeModels(exe, true);
        if (!alive) return;
        setOpencodeModels(models);
        setOpencodeModelsStatus('loaded');
      } catch (error) {
        if (!alive) return;
        setOpencodeModelsError(error instanceof Error ? error.message : String(error));
        setOpencodeModelsStatus('error');
      }
    })();
    return () => {
      alive = false;
    };
  }, [useOpencode, prefs?.codingAgentExe]);

  useEffect(() => {
    if (
      !prefs ||
      !isSandboxPermissionProvider(provider) ||
      (prefs.codingAgentPermissionMode !== 'default' &&
        prefs.codingAgentPermissionMode !== 'bypassPermissions')
    ) {
      return;
    }
    void savePrefs({ ...prefs, codingAgentPermissionMode: 'plan' });
  }, [prefs, provider, savePrefs]);

  const refreshOpencodeModels = async () => {
    setOpencodeModelsStatus('loading');
    setOpencodeModelsError('');
    try {
      const models = await codingAgentListOpencodeModels(prefs?.codingAgentExe ?? undefined, true);
      setOpencodeModels(models);
      setOpencodeModelsStatus('loaded');
    } catch (error) {
      setOpencodeModelsError(error instanceof Error ? error.message : String(error));
      setOpencodeModelsStatus('error');
    }
  };

  // Windows/macOS 共用 Core Agent 流程。

  if (!prefs) {
    return (
      <Card>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
      </Card>
    );
  }

  const enabled = prefs.codingAgentEnabled;

  return (
    <Card>
      <SettingRow
        label={t('settings.codingAgent.enable')}
        desc={t('settings.codingAgent.hotkeyHint')}
      >
        <Toggle
          on={enabled}
          onToggle={(next) => void savePrefs({ ...prefs, codingAgentEnabled: next })}
        />
      </SettingRow>

      {enabled && (
        <>
          {/* "Hold-to-talk key" config moved to General → Shortcuts to avoid duplication. This section keeps only backend/model and other advanced items. */}
          <SettingRow label={t('settings.codingAgent.provider')}>
            <SelectLite
              value={prefs.codingAgentProvider}
              onChange={(v) => {
                const nextProvider = v as CodingAgentProviderId;
                void savePrefs({
                  ...prefs,
                  codingAgentProvider: nextProvider,
                  codingAgentModel: null,
                  codingAgentExe: null,
                  codingAgentPermissionMode: normalizePermissionMode(
                    nextProvider,
                    prefs.codingAgentPermissionMode,
                  ),
                });
              }}
              options={PROVIDERS}
              ariaLabel={t('settings.codingAgent.provider')}
              style={{ ...inputStyle, maxWidth: 240 }}
            />
          </SettingRow>

          {/* OpenCode backend: show install/login status. issue #579. */}
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

          {/* Codex / dsh: installed or not + version. Warn-colored hint when missing. */}
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

          {/* Guardrail difference: these two have no per-command deny list, so the approval card never appears. Don't let users assume it does. */}
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
              onChange={(v) =>
                void savePrefs({
                  ...prefs,
                  codingAgentPermissionMode: v as CodingAgentPermissionMode,
                })
              }
              options={permissionModesForProvider(provider).map((m) => ({
                value: m,
                label: t(
                  isSandboxPermissionProvider(provider)
                    ? `settings.codingAgent.codexMode.${m === 'acceptEdits' ? 'workspaceWrite' : 'plan'}`
                    : `settings.codingConsole.mode.${m}`,
                ),
              }))}
              ariaLabel={t('settings.codingConsole.permissionMode')}
              style={{ ...inputStyle, maxWidth: 240 }}
            />
          </SettingRow>

          {/* dsh's headless profile has no --model: the model is decided by the profile, so don't offer a fake switch here. */}
          {useDsh && (
            <div
              style={{
                fontSize: 12,
                lineHeight: 1.6,
                color: 'var(--ol-ink-4)',
                margin: '-4px 0 8px',
              }}
            >
              {t('settings.codingAgent.dshModelHint')}
            </div>
          )}

          {!useDsh && (
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
                  // Codex model names are bare (gpt-5 / o3 / any name from a self-hosted gateway);
                  // they can't be enumerated, so use free text. Empty = use the settings in ~/.codex/config.toml.
                  <input
                    type="text"
                    value={prefs.codingAgentModel ?? ''}
                    placeholder={t('settings.codingAgent.codexModelPlaceholder')}
                    spellCheck={false}
                    aria-label={t('settings.codingAgent.model')}
                    onChange={(e) => {
                      const v = e.target.value.trim();
                      void savePrefs({ ...prefs, codingAgentModel: v === '' ? null : v });
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
                    onChange={(v) =>
                      void savePrefs({ ...prefs, codingAgentModel: v === '' ? null : v })
                    }
                    options={
                      useOpencode
                        ? [
                            // Empty = use the OpenCode CLI default model.
                            { value: '', label: t('settings.codingAgent.opencodeModelDefault') },
                            // Keep a selected model that's missing from the fetched list, so the selection doesn't vanish.
                            ...(prefs.codingAgentModel?.includes('/') &&
                            !opencodeModels.includes(prefs.codingAgentModel)
                              ? [{ value: prefs.codingAgentModel, label: prefs.codingAgentModel }]
                              : []),
                            ...opencodeModels.map((model) => ({ value: model, label: model })),
                          ]
                        : [
                            // Empty = use the CLI default model; keep it as an option so a specific model choice can be undone.
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
          )}

          {useOpencode && opencode?.installed && opencodeModelsStatus !== 'idle' && (
            <div
              role={opencodeModelsStatus === 'error' ? 'alert' : 'status'}
              style={{
                fontSize: 12,
                lineHeight: 1.6,
                color:
                  opencodeModelsStatus === 'error' ? 'var(--ol-warn, #b8860b)' : 'var(--ol-ink-4)',
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

          <SettingRow
            label={t('settings.codingConsole.workdir')}
            desc={t('settings.codingConsole.workdirDesc')}
          >
            <input
              type="text"
              value={prefs.codingAgentWorkdir ?? ''}
              placeholder={t('settings.codingConsole.workdirPlaceholder')}
              spellCheck={false}
              onChange={(e) => {
                const v = e.target.value.trim();
                void savePrefs({ ...prefs, codingAgentWorkdir: v === '' ? null : v });
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
              onChange={(e) => {
                const v = e.target.value.trim();
                void savePrefs({ ...prefs, codingAgentExe: v === '' ? null : v });
              }}
              style={inputStyle}
            />
          </SettingRow>

          <SettingRow
            label={t('settings.codingAgent.openPanel')}
            desc={t('settings.codingAgent.openPanelHint')}
          >
            <div>
              <button
                type="button"
                disabled={openPanelBusy}
                aria-busy={openPanelBusy}
                onClick={() => void openPanel()}
                style={{ ...inputStyle, width: 'auto', cursor: openPanelBusy ? 'wait' : 'pointer' }}
              >
                {openPanelBusy ? t('common.loading') : t('settings.codingAgent.openPanelAction')}
              </button>
              {openPanelError && (
                <div role="alert" style={{ fontSize: 11, color: 'var(--ol-err)', marginTop: 4 }}>
                  {t('common.operationFailed')}: {openPanelError}
                </div>
              )}
            </div>
          </SettingRow>
        </>
      )}
    </Card>
  );
}
