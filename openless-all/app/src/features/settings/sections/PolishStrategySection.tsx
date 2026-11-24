import { useEffect, useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { copyTextToClipboard } from '../../../api/files';
import { PageSkeleton } from '../../../components/ui/PageSkeleton';
import { useHotkeySettings } from '../../../data/preferences';
import { buildPolishStrategyAgentSkill } from '../../../lib/polishStrategyAgentSkill';
import { Button } from '../../../ui/controls/Button';
import { SettingRow, Toggle } from '../../../ui/controls/SettingRow';
import { FieldFrame } from '../../../ui/controls/TextField';
import { useCopyFeedback } from '../../../ui/feedback/useCopyFeedback';
import { Card } from '../../../ui/layout/Card';
import { useInteraction } from '../../../ui/overlays/InteractionProvider';
import { PresenceModal } from '../../../ui/overlays/Modal';
import './polish-strategy.css';

export function PolishStrategySection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const { notify } = useInteraction('polish-strategy');
  const editorId = useId();
  const titleId = useId();
  const editorRef = useRef<HTMLTextAreaElement>(null);
  const [editing, setEditing] = useState(false);
  const { copied, copy } = useCopyFeedback(copyTextToClipboard, () =>
    notify({
      key: 'copy',
      state: 'failed',
      message: t('settings.recording.polishStrategySkillCopyFailed'),
    }),
  );

  useEffect(() => {
    if (!prefs?.polishStrategyEnabled) setEditing(false);
  }, [prefs?.polishStrategyEnabled]);

  if (!prefs) return <PageSkeleton rows={1} />;

  return (
    <>
      <Card className="ol-polish-strategy">
        <SettingRow
          className="ol-polish-strategy-toggle"
          label={t('settings.recording.polishStrategyEnabledLabel')}
          desc={t('settings.recording.polishStrategyEnabledDesc')}
        >
          <Toggle
            on={prefs.polishStrategyEnabled}
            onToggle={(polishStrategyEnabled) =>
              void updatePrefs((current) => ({ ...current, polishStrategyEnabled }))
            }
          />
        </SettingRow>
        {prefs.polishStrategyEnabled && (
          <SettingRow
            className="ol-polish-strategy-edit-row"
            label={t('settings.recording.polishStrategyScriptLabel')}
          >
            <Button size="sm" icon="pencil" onClick={() => setEditing(true)}>
              {t('settings.recording.polishStrategyEdit')}
            </Button>
          </SettingRow>
        )}
      </Card>
      <PresenceModal
        open={editing && prefs.polishStrategyEnabled}
        onClose={() => setEditing(false)}
        width="min(760px, 100%)"
        labelledBy={titleId}
        initialFocus={editorRef}
        motionVariant="side-panel"
        overlayClassName="ol-polish-strategy-overlay"
        cardClassName="ol-polish-strategy-panel"
        render={() => (
          <>
            <header className="ol-polish-strategy-panel-header">
              <div>
                <h2 id={titleId}>{t('settings.recording.polishStrategyEditorTitle')}</h2>
                <span>{t('modal.autoSaveHint')}</span>
              </div>
              <Button
                size="sm"
                icon="close"
                ariaLabel={t('common.close')}
                onClick={() => setEditing(false)}
              />
            </header>
            <div className="ol-polish-strategy-editor">
              <div className="ol-polish-strategy-toolbar">
                <label htmlFor={editorId}>
                  {t('settings.recording.polishStrategyScriptLabel')}
                </label>
                <Button
                  size="sm"
                  icon={copied ? 'check' : 'copy'}
                  ariaLabel={t(
                    copied
                      ? 'settings.recording.polishStrategySkillCopied'
                      : 'settings.recording.polishStrategySkillCopy',
                  )}
                  onClick={() =>
                    void copy('strategy', buildPolishStrategyAgentSkill(prefs.polishStrategyScript))
                  }
                >
                  {t('settings.recording.polishStrategySkillCopy')}
                </Button>
                <span className="ol-sr-only" role="status">
                  {copied ? t('settings.recording.polishStrategySkillCopied') : ''}
                </span>
              </div>
              <FieldFrame className="ol-polish-strategy-field">
                <textarea
                  id={editorId}
                  ref={editorRef}
                  className="ol-field-control"
                  value={prefs.polishStrategyScript}
                  onChange={(event) => {
                    const polishStrategyScript = event.target.value.slice(0, 32_768);
                    void updatePrefs((current) => ({ ...current, polishStrategyScript }));
                  }}
                  placeholder={t('settings.recording.polishStrategyScriptPlaceholder')}
                  spellCheck={false}
                  autoCapitalize="none"
                  autoCorrect="off"
                  rows={12}
                  maxLength={32_768}
                />
              </FieldFrame>
            </div>
          </>
        )}
      />
    </>
  );
}
