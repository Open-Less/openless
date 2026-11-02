import { X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { ReactNode } from 'react';
import type { EditPlanFormat, UserPreferences } from '../../lib/types';
import { PresenceModal } from '../../components/ui/Modal';
import { Segmented } from '../../components/ui/Segmented';
import { useMobileLayout } from '../../lib/useMobileLayout';
import { Toggle, inputStyle } from './shared';

const editPlanFormatOptions: Array<{ value: EditPlanFormat }> = [
  { value: 'xml' },
  { value: 'json' },
];

interface SelectionWorkspaceExtraControlsDialogProps {
  open: boolean;
  autoIntent: boolean;
  keywordsText: string;
  editPlanFormat: EditPlanFormat;
  editSystemPrompt: string;
  updatePrefs: (
    next: UserPreferences | ((current: UserPreferences) => UserPreferences),
  ) => Promise<void>;
  onClose: () => void;
}

export function SelectionWorkspaceExtraControlsDialog({
  open,
  autoIntent,
  keywordsText,
  editPlanFormat,
  editSystemPrompt,
  updatePrefs,
  onClose,
}: SelectionWorkspaceExtraControlsDialogProps) {
  const { t } = useTranslation();

  return (
    <PresenceModal
      open={open}
      onClose={onClose}
      width="min(680px, 100%)"
      labelledBy="selection-extra-controls-title"
      style={{ padding: 0, overflow: 'hidden', maxHeight: 'min(720px, 88vh)' }}
      render={() => (
        <div style={{ display: 'flex', flexDirection: 'column', maxHeight: 'min(720px, 88vh)' }}>
          <header
            style={{
              display: 'flex',
              alignItems: 'flex-start',
              justifyContent: 'space-between',
              gap: 16,
              padding: '22px 24px 16px',
              borderBottom: '0.5px solid var(--ol-line-soft)',
            }}
          >
            <div>
              <h2
                id="selection-extra-controls-title"
                style={{ margin: 0, fontSize: 18, color: 'var(--ol-ink)' }}
              >
                {t('settings.selectionWorkspace.extraControls')}
              </h2>
              <p
                style={{
                  margin: '7px 0 0',
                  color: 'var(--ol-ink-3)',
                  fontSize: 12.5,
                  lineHeight: 1.5,
                }}
              >
                {t('settings.selectionWorkspace.extraControlsDesc')}
              </p>
            </div>
            <button
              type="button"
              aria-label={t('common.close')}
              onClick={onClose}
              style={{
                border: 0,
                background: 'transparent',
                color: 'var(--ol-ink-3)',
                cursor: 'pointer',
                fontSize: 20,
                lineHeight: 1,
                padding: 4,
              }}
            >
              <X size={18} />
            </button>
          </header>

          <div className="ol-thinscroll" style={{ overflowY: 'auto', padding: '4px 24px 18px' }}>
            <ControlBlock
              label={t('settings.selectionWorkspace.autoIntent')}
              description={t('settings.selectionWorkspace.autoIntentDesc')}
            >
              <Toggle
                on={autoIntent}
                label={t('settings.selectionWorkspace.autoIntent')}
                onToggle={(next) =>
                  void updatePrefs((current) => ({
                    ...current,
                    selectionVoiceIntentMode: next ? 'auto' : 'heuristic',
                  }))
                }
              />
            </ControlBlock>

            {!autoIntent && (
              <ControlBlock
                label={t('settings.selectionWorkspace.editKeywords')}
                description={t('settings.selectionWorkspace.editKeywordsDesc')}
              >
                <textarea
                  aria-label={t('settings.selectionWorkspace.editKeywords')}
                  value={keywordsText}
                  onChange={(event) => {
                    const lines = event.target.value
                      .split(/\n/)
                      .map((line) => line.trim())
                      .filter(Boolean);
                    void updatePrefs((current) => ({
                      ...current,
                      selectionVoiceEditKeywords: lines,
                      selectionVoiceIntentMode: 'heuristic',
                    }));
                  }}
                  rows={5}
                  style={{
                    ...inputStyle,
                    width: '100%',
                    maxWidth: 'none',
                    minHeight: 104,
                    resize: 'vertical',
                    lineHeight: 1.5,
                    padding: '9px 10px',
                  }}
                />
              </ControlBlock>
            )}

            <ControlBlock
              label={t('settings.selectionWorkspace.editPlanFormat')}
              description={t('settings.selectionWorkspace.editPlanFormatDesc')}
            >
              <Segmented
                label={t('settings.selectionWorkspace.editPlanFormat')}
                value={editPlanFormat}
                onChange={(next) =>
                  void updatePrefs((current) => ({
                    ...current,
                    selectionVoiceEditPlanFormat: next,
                  }))
                }
                options={editPlanFormatOptions.map((option) => ({
                  value: option.value,
                  label: t(
                    `settings.selectionWorkspace.editPlanFormat${option.value === 'xml' ? 'Xml' : 'Json'}`,
                  ),
                }))}
              />
            </ControlBlock>

            <ControlBlock
              label={t('settings.selectionWorkspace.editSystemPrompt')}
              description={t('settings.selectionWorkspace.editSystemPromptDesc')}
            >
              <div style={{ display: 'flex', flexDirection: 'column', gap: 8, width: '100%' }}>
                <textarea
                  aria-label={t('settings.selectionWorkspace.editSystemPrompt')}
                  value={editSystemPrompt}
                  placeholder={t('settings.selectionWorkspace.editSystemPromptPlaceholder')}
                  onChange={(event) => {
                    const next = event.target.value;
                    void updatePrefs((current) => ({
                      ...current,
                      selectionVoiceEditSystemPrompt: next,
                    }));
                  }}
                  rows={7}
                  style={{
                    ...inputStyle,
                    width: '100%',
                    maxWidth: 'none',
                    minHeight: 132,
                    resize: 'vertical',
                    lineHeight: 1.5,
                    padding: '9px 10px',
                  }}
                />
                <button
                  type="button"
                  onClick={() =>
                    void updatePrefs((current) => ({
                      ...current,
                      selectionVoiceEditSystemPrompt: '',
                    }))
                  }
                  style={{
                    alignSelf: 'flex-start',
                    border: '0.5px solid var(--ol-line-strong)',
                    borderRadius: 7,
                    background: 'var(--ol-control-solid)',
                    color: 'var(--ol-ink-2)',
                    padding: '6px 10px',
                    fontSize: 12,
                    fontFamily: 'inherit',
                    cursor: 'pointer',
                  }}
                >
                  {t('settings.selectionWorkspace.editSystemPromptReset')}
                </button>
              </div>
            </ControlBlock>
          </div>

          <footer
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              gap: 12,
              padding: '14px 24px 18px',
              borderTop: '0.5px solid var(--ol-line-soft)',
            }}
          >
            <span style={{ color: 'var(--ol-ink-3)', fontSize: 12 }}>
              {t('settings.selectionWorkspace.extraControlsSaveHint')}
            </span>
            <button
              type="button"
              onClick={onClose}
              style={{
                border: '0.5px solid var(--ol-line-strong)',
                borderRadius: 7,
                background: 'var(--ol-control-solid)',
                color: 'var(--ol-ink)',
                padding: '7px 14px',
                fontSize: 12.5,
                fontFamily: 'inherit',
                cursor: 'pointer',
              }}
            >
              {t('settings.selectionWorkspace.extraControlsDone')}
            </button>
          </footer>
        </div>
      )}
    />
  );
}

function ControlBlock({
  label,
  description,
  children,
}: {
  label: string;
  description: string;
  children: ReactNode;
}) {
  const mobile = useMobileLayout();
  return (
    <section
      style={{
        display: 'grid',
        gridTemplateColumns: mobile ? 'minmax(0, 1fr)' : 'minmax(0, 210px) minmax(0, 1fr)',
        gap: 18,
        alignItems: 'start',
        padding: '16px 0',
        borderBottom: '0.5px solid var(--ol-line-soft)',
      }}
    >
      <div>
        <div
          style={{
            color: 'var(--ol-ink)',
            fontSize: 14,
            fontWeight: 'var(--ol-font-weight-medium)',
          }}
        >
          {label}
        </div>
        <p style={{ margin: '6px 0 0', color: 'var(--ol-ink-3)', fontSize: 12, lineHeight: 1.5 }}>
          {description}
        </p>
      </div>
      <div style={{ minWidth: 0, display: 'flex', alignItems: 'flex-start' }}>{children}</div>
    </section>
  );
}
