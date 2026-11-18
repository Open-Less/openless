import { AnimatePresence, motion } from 'framer-motion';
import { type CSSProperties } from 'react';
import type { PolishMode } from '../../api/contracts';
import { Icon } from '../../components/Icon';
import { StylePackIconPicker } from './StylePackIconPicker';
import { ChoiceChips } from '../../components/ui/ChoiceChips';
import { ExpandableText } from '../../components/ui/Disclosure';
import { DismissibleCard } from '../../components/ui/DismissibleCard';
import { getStylePackPresentation } from '../../lib/stylePackPresentation';
import { Button as Btn } from '../../ui/controls/Button';
import { Card } from '../../ui/layout/Card';
import { PageHeader } from '../../ui/layout/PageHeader';
import { Pill } from '../../ui/controls/Pill';
import { useStylePackWorkspace } from './useStylePackWorkspace';

function modeTone(mode: PolishMode): 'default' | 'blue' | 'ok' | 'outline' | 'dark' {
  return mode === 'raw' ? 'outline' : 'default';
}

export function Style() {
  const {
    t,
    mobile,
    reducedMotion,
    stackLayout,
    marketplacePrefs,
    canPublish,
    packs,
    setPacks,
    workflowView,
    setWorkflowView,
    draft,
    busy,
    editorOpen,
    runtimePreview,
    runtimePreviewError,
    showSaveStatus,
    rawPack,
    bodyPacks,
    selectionPolishPackId,
    dirty,
    discardDraftChanges,
    closeEditor,
    openEditorForPack,
    patchDraft,
    patchExample,
    appendExample,
    removeExample,
    handleSave,
    handleActivate,
    handleActivateSelectionStyle,
    handleResetBuiltin,
    handleDeleteImportedPack,
    handleDeleteImported,
    handleCreateFromTemplate,
    handleImportZip,
    handlePublishToMarketplace,
    handleExportZip,
  } = useStylePackWorkspace();
  const selectionPromptEditor = (
    <label style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 12,
          flexWrap: 'wrap',
        }}
      >
        <span
          style={{
            fontSize: 12,
            fontWeight: 'var(--ol-font-weight-semibold)',
            color: 'var(--ol-ink)',
          }}
        >
          {t('style.pack.selectionPromptTitle')}
        </span>
        <Pill tone="default" size="sm">
          {t('style.pack.selectionChars', { count: draft?.selectionPrompt.length ?? 0 })}
        </Pill>
      </div>
      <span style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.55 }}>
        {t('style.pack.selectionPromptHint')}
      </span>
      <textarea
        value={draft?.selectionPrompt ?? ''}
        onChange={(event) => patchDraft({ selectionPrompt: event.target.value })}
        style={{ ...textareaStyle, minHeight: 150 }}
      />
    </label>
  );

  const voiceEditPromptEditor = (
    <label style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 12,
          flexWrap: 'wrap',
        }}
      >
        <span
          style={{
            fontSize: 12,
            fontWeight: 'var(--ol-font-weight-semibold)',
            color: 'var(--ol-ink)',
          }}
        >
          {t('style.pack.voiceEditPromptTitle')}
        </span>
        <Pill tone="default" size="sm">
          {t('style.pack.selectionChars', { count: draft?.voiceEditPrompt?.length ?? 0 })}
        </Pill>
      </div>
      <span style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.55 }}>
        {t('style.pack.voiceEditPromptHint')}
      </span>
      <textarea
        value={draft?.voiceEditPrompt ?? ''}
        placeholder={t('style.pack.voiceEditPromptPlaceholder')}
        onChange={(event) => patchDraft({ voiceEditPrompt: event.target.value })}
        style={{ ...textareaStyle, minHeight: 150 }}
      />
    </label>
  );

  const selectionWorkflowEditors = (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
      {selectionPromptEditor}
      {voiceEditPromptEditor}
    </div>
  );

  const dictationPromptEditor = (
    <label style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 12,
          flexWrap: 'wrap',
        }}
      >
        <span
          style={{
            fontSize: 12,
            fontWeight: 'var(--ol-font-weight-semibold)',
            color: 'var(--ol-ink)',
          }}
        >
          {t('style.pack.dictationPromptTitle')}
        </span>
        <Pill tone="default" size="sm">
          {t('style.pack.promptChars', { count: draft?.prompt.length ?? 0 })}
        </Pill>
      </div>
      <span style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.55 }}>
        {t('style.pack.dictationPromptHint')}
      </span>
      <textarea
        value={draft?.prompt ?? ''}
        onChange={(event) => patchDraft({ prompt: event.target.value })}
        style={{ ...textareaStyle, minHeight: 210 }}
      />
    </label>
  );

  return (
    <>
      <PageHeader
        kicker={t('style.pack.kicker')}
        title={t('style.pack.title')}
        desc={t('style.pack.desc')}
        right={
          <div
            className="ol-flex-row ol-flex-push"
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 8,
              flexWrap: 'wrap',
              justifyContent: stackLayout ? 'flex-start' : 'flex-end',
              marginTop: stackLayout ? 0 : 40,
              width: stackLayout ? '100%' : undefined,
            }}
          >
            <Btn
              variant="blue"
              icon="archive"
              onClick={() => void handleImportZip()}
              disabled={busy === 'importing'}
            >
              {busy === 'importing' ? t('common.loading') : t('style.pack.importZip')}
            </Btn>
          </div>
        }
      />

      <Card
        padding={0}
        style={{
          overflow: 'hidden',
          flex: '1 1 0',
          minHeight: 0,
          display: 'flex',
          flexDirection: 'column',
        }}
      >
        <div
          style={{
            padding: stackLayout ? 14 : 18,
            borderBottom: '0.5px solid var(--ol-line)',
            flexShrink: 0,
          }}
        >
          <div
            className="ol-flex-row"
            style={{
              display: 'flex',
              alignItems: 'flex-start',
              justifyContent: 'space-between',
              gap: 12,
              flexWrap: 'wrap',
              flexDirection: stackLayout ? 'column' : 'row',
            }}
          >
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 12,
                flexWrap: 'wrap',
                minWidth: 0,
              }}
            >
              <div>
                <div
                  style={{
                    fontSize: 15,
                    fontWeight: 'var(--ol-font-weight-semibold)',
                    color: 'var(--ol-ink)',
                  }}
                >
                  {workflowView === 'dictation'
                    ? t('style.pack.listTitle')
                    : t('style.pack.selectionListTitle')}
                </div>
                <div
                  style={{ fontSize: 12, color: 'var(--ol-ink-3)', marginTop: 4, maxWidth: 760 }}
                >
                  {workflowView === 'dictation'
                    ? t('style.pack.listDesc')
                    : t('style.pack.selectionListDesc')}
                </div>
              </div>
              {rawPack && workflowView === 'dictation' && (
                <button
                  type="button"
                  onClick={() => void handleActivate(rawPack)}
                  disabled={rawPack.active || busy === 'activating'}
                  title={getStylePackPresentation(rawPack, t).name}
                  style={{
                    display: 'inline-flex',
                    alignItems: 'center',
                    gap: 6,
                    padding: '6px 12px',
                    borderRadius: 'var(--ol-radius)',
                    border: '0.5px solid',
                    borderColor: 'var(--ol-line-strong)',
                    background: rawPack.active ? 'var(--ol-style-card-bg-active)' : 'transparent',
                    color: 'var(--ol-ink-2)',
                    fontSize: 12.5,
                    fontWeight: rawPack.active
                      ? 'var(--ol-font-weight-semibold)'
                      : 'var(--ol-font-weight-medium)',
                    whiteSpace: 'nowrap',
                    cursor: rawPack.active ? 'default' : 'pointer',
                    transition:
                      'border-color 0.16s var(--ol-motion-quick), background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick)',
                  }}
                >
                  <span>{getStylePackPresentation(rawPack, t).name}</span>
                  {rawPack.active && (
                    <span style={{ fontSize: 11, opacity: 0.85 }}>·{t('style.pack.active')}</span>
                  )}
                </button>
              )}
            </div>
            <div
              className="ol-flex-row ol-flex-split"
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 8,
                flexWrap: 'wrap',
                minWidth: 0,
                width: stackLayout ? '100%' : undefined,
              }}
            >
              <ChoiceChips
                label={t('style.pack.listTitle')}
                value={workflowView}
                onChange={setWorkflowView}
                options={[
                  { value: 'dictation', label: t('style.pack.dictationTab') },
                  { value: 'selection', label: t('style.pack.selectionTab') },
                ]}
              />
              <Pill tone="outline">{t('style.pack.listCount', { count: packs.length })}</Pill>
            </div>
          </div>
        </div>
        <motion.div
          layoutScroll
          className="ol-thinscroll"
          style={{ padding: 18, overflow: 'auto', flex: '1 1 0', minHeight: 0 }}
        >
          <div
            className="ol-grid-auto-cards ol-grid-12 ol-grid-cards"
            data-columns={stackLayout ? '1' : undefined}
            style={{ position: 'relative' }}
          >
            <AnimatePresence mode="popLayout" initial={false}>
              {bodyPacks.map((pack) => {
                const isBuiltin = pack.kind === 'builtin';
                const presentation = getStylePackPresentation(pack, t);
                const isCurrentForView =
                  workflowView === 'selection' ? pack.id === selectionPolishPackId : pack.active;
                return (
                  <DismissibleCard
                    key={pack.id}
                    dismissible={!isBuiltin && busy !== 'deleting'}
                    resetOnDismiss
                    onDismiss={() => void handleDeleteImportedPack(pack)}
                    data-ol-style-pack={pack.id}
                    data-active={isCurrentForView ? 'true' : undefined}
                    layout={reducedMotion ? false : 'position'}
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0 }}
                    transition={{
                      layout: { duration: reducedMotion ? 0 : 0.22, ease: [0.22, 1, 0.36, 1] },
                      opacity: { duration: reducedMotion ? 0 : 0.12 },
                    }}
                    style={{
                      display: 'flex',
                      flexDirection: 'column',
                      textAlign: 'left',
                      position: 'relative',
                      border: 0,
                      background: isCurrentForView ? 'var(--ol-blue-soft)' : 'var(--ol-surface)',
                      borderRadius: 'var(--ol-r-lg)',
                      padding: 16,
                      boxShadow: 'none',
                      cursor: 'default',
                      minHeight: 204,
                    }}
                  >
                    <div
                      style={{
                        display: 'flex',
                        alignItems: 'flex-start',
                        justifyContent: 'space-between',
                        gap: 10,
                        marginBottom: 10,
                      }}
                    >
                      <div style={{ minWidth: 0 }}>
                        <div
                          style={{
                            display: 'flex',
                            alignItems: 'center',
                            gap: 8,
                            flexWrap: 'wrap',
                          }}
                        >
                          <div
                            style={{
                              fontSize: 15,
                              fontWeight: 'var(--ol-font-weight-semibold)',
                              color: 'var(--ol-style-card-ink)',
                            }}
                          >
                            {presentation.name}
                          </div>
                          <Pill tone="outline" size="sm">
                            {isBuiltin ? t('style.pack.builtin') : t('style.pack.imported')}
                          </Pill>
                          {pack.originAuthorLogin &&
                            pack.originAuthorLogin !==
                              (marketplacePrefs?.marketplaceDevLogin ?? '').trim() && (
                              <span
                                title={t('style.pack.derivativeBadge', {
                                  login: pack.originAuthorLogin,
                                })}
                              >
                                <Pill tone="ok" size="sm">
                                  {t('style.pack.derivativeBadge', {
                                    login: pack.originAuthorLogin,
                                  })}
                                </Pill>
                              </span>
                            )}
                          {isCurrentForView && (
                            <span
                              style={{
                                display: 'inline-flex',
                                alignItems: 'center',
                                gap: 4,
                                color: 'var(--ol-blue)',
                              }}
                            >
                              <Icon name="check" size={14} />
                              <Pill tone="outline" size="sm">
                                {t('style.pack.current')}
                              </Pill>
                            </span>
                          )}
                        </div>
                        <ExpandableText text={presentation.description || presentation.name} />
                      </div>
                      <StylePackIconPicker
                        pack={pack}
                        onSaved={(saved) =>
                          setPacks((current) =>
                            current.map((item) =>
                              item.id === saved.id
                                ? { ...item, iconPath: saved.iconPath, updatedAt: saved.updatedAt }
                                : item,
                            ),
                          )
                        }
                        onStatus={(failed, message) =>
                          showSaveStatus(failed ? 'failed' : 'saved', message, true)
                        }
                      />
                    </div>

                    <div
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        gap: 8,
                        flexWrap: 'wrap',
                        minHeight: 24,
                        marginBottom: 12,
                      }}
                    >
                      <Pill tone="default" size="sm">
                        {workflowView === 'selection'
                          ? t('style.pack.writtenPolish')
                          : t(`style.modes.${pack.baseMode}.name`)}
                      </Pill>
                      {presentation.tags
                        .filter(
                          (tag) =>
                            tag !== presentation.name &&
                            tag !==
                              (workflowView === 'selection'
                                ? t('style.pack.writtenPolish')
                                : t(`style.modes.${pack.baseMode}.name`)),
                        )
                        .slice(0, 1)
                        .map((tag) => (
                          <Pill key={`${pack.id}-${tag}`} tone="default" size="sm">
                            {tag}
                          </Pill>
                        ))}
                    </div>

                    <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', marginTop: 'auto' }}>
                      <Btn
                        size="sm"
                        variant={isCurrentForView ? 'soft' : 'blue'}
                        disabled={isCurrentForView || busy === 'activating'}
                        onClick={() =>
                          void (workflowView === 'selection'
                            ? handleActivateSelectionStyle(pack)
                            : handleActivate(pack))
                        }
                      >
                        {isCurrentForView
                          ? t('style.pack.current')
                          : workflowView === 'selection'
                            ? t('style.pack.useForSelection')
                            : t('style.pack.activate')}
                      </Btn>
                      <Btn
                        size="sm"
                        variant="ghost"
                        icon="archive"
                        disabled={busy === 'exporting'}
                        onClick={() => void handleExportZip(pack)}
                      >
                        {t('style.pack.exportShort')}
                      </Btn>
                      <Btn
                        size="sm"
                        variant="ghost"
                        icon="pencil"
                        disabled={isBuiltin}
                        onClick={() => openEditorForPack(pack)}
                      >
                        {t('style.pack.edit')}
                      </Btn>
                      {!isBuiltin && (
                        <button
                          type="button"
                          className="ol-style-pack-delete"
                          onClick={() => void handleDeleteImportedPack(pack)}
                          disabled={busy === 'deleting'}
                          aria-label={t('style.pack.deleteImported')}
                          title={t('style.pack.deleteImported')}
                        >
                          <Icon name="trash" size={14} />
                        </button>
                      )}
                    </div>
                  </DismissibleCard>
                );
              })}
              <motion.button
                key="add-new-pack-btn"
                layout={reducedMotion ? false : 'position'}
                initial={false}
                transition={{
                  layout: { duration: reducedMotion ? 0 : 0.22, ease: [0.22, 1, 0.36, 1] },
                }}
                type="button"
                disabled={busy === 'creating'}
                aria-label={t('style.pack.addPackTileTitle')}
                onClick={() => void handleCreateFromTemplate()}
                style={{
                  display: 'flex',
                  flexDirection: 'column',
                  alignItems: 'center',
                  justifyContent: 'center',
                  gap: 8,
                  textAlign: 'center',
                  border: '0.5px dashed var(--ol-line-strong)',
                  borderRadius: 'var(--ol-r-lg)',
                  padding: 16,
                  background: 'transparent',
                  color: 'var(--ol-ink-3)',
                  cursor: busy === 'creating' ? 'wait' : 'pointer',
                  opacity: busy === 'creating' ? 0.55 : 1,
                  minHeight: 204,
                  transition:
                    'border-color 0.16s var(--ol-motion-quick), background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick)',
                }}
              >
                <div
                  style={{
                    width: 44,
                    height: 44,
                    borderRadius: 'var(--ol-radius)',
                    display: 'inline-flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    background: 'var(--ol-surface-2)',
                    color: 'var(--ol-ink-2)',
                  }}
                >
                  <Icon name="plus" size={22} />
                </div>
                <div
                  style={{
                    fontSize: 14,
                    fontWeight: 'var(--ol-font-weight-semibold)',
                    color: 'var(--ol-ink-2)',
                  }}
                >
                  {t('style.pack.addPackTileTitle')}
                </div>
                <div
                  style={{
                    fontSize: 12,
                    color: 'var(--ol-ink-4)',
                    lineHeight: 1.55,
                    maxWidth: 220,
                  }}
                >
                  {t('style.pack.addPackTileHint')}
                </div>
              </motion.button>
            </AnimatePresence>
          </div>
        </motion.div>
      </Card>

      <AnimatePresence>
        {editorOpen && (
          <>
            <motion.div
              aria-hidden="true"
              className="ol-modal-backdrop"
              onClick={closeEditor}
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.2, ease: 'easeOut' }}
              style={{
                position: 'fixed',
                inset: 0,
                background: 'var(--ol-dialog-backdrop)',
                ...(mobile
                  ? {}
                  : {
                      backdropFilter: 'blur(8px) saturate(140%)',
                      WebkitBackdropFilter: 'blur(8px) saturate(140%)',
                    }),
                zIndex: mobile ? 70 : 40,
              }}
            />
            <motion.div
              role="dialog"
              aria-modal="true"
              aria-label={t('style.pack.editorTitle')}
              initial={{ x: '100%', opacity: 0 }}
              animate={{ x: 0, opacity: 1 }}
              exit={{ x: '100%', opacity: 0 }}
              transition={{ type: 'spring', damping: 36, stiffness: 280 }}
              style={
                mobile
                  ? {
                      position: 'fixed',
                      inset: 0,
                      width: '100%',
                      zIndex: 71,
                    }
                  : {
                      position: 'fixed',
                      top: 16,
                      right: 16,
                      bottom: 16,
                      width: 'min(760px, calc(100vw - 32px))',
                      zIndex: 41,
                    }
              }
            >
              <Card
                padding={0}
                style={{
                  height: '100%',
                  display: 'grid',
                  gridTemplateRows: 'auto minmax(0, 1fr)',
                  overflow: 'hidden',
                  boxShadow: mobile ? 'none' : 'var(--ol-dialog-shadow)',
                  borderRadius: mobile ? 0 : 'var(--ol-dialog-radius)',
                }}
              >
                <div style={{ padding: 18, borderBottom: '0.5px solid var(--ol-line)' }}>
                  <div
                    style={{
                      display: 'flex',
                      alignItems: 'flex-start',
                      justifyContent: 'space-between',
                      gap: 12,
                    }}
                  >
                    <div style={{ minWidth: 0 }}>
                      <div
                        style={{
                          fontSize: 15,
                          fontWeight: 'var(--ol-font-weight-semibold)',
                          color: 'var(--ol-ink)',
                        }}
                      >
                        {t('style.pack.editorTitle')}
                      </div>
                      <div
                        style={{
                          fontSize: 12,
                          color: 'var(--ol-ink-3)',
                          marginTop: 4,
                          lineHeight: 1.6,
                        }}
                      >
                        {workflowView === 'dictation'
                          ? t('style.pack.dictationPromptEditorDesc')
                          : t('style.pack.selectionPromptEditorDesc')}
                      </div>
                    </div>
                    <button
                      type="button"
                      onClick={closeEditor}
                      aria-label={t('style.pack.closeEditor')}
                      style={{
                        width: 28,
                        height: 28,
                        borderRadius: 'var(--ol-radius)',
                        border: 0,
                        background: 'transparent',
                        color: 'var(--ol-ink-3)',
                        display: 'inline-flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                        flexShrink: 0,
                      }}
                    >
                      <Icon name="close" size={14} />
                    </button>
                  </div>
                </div>

                {!draft ? (
                  <div
                    style={{ padding: 28, color: 'var(--ol-ink-3)', fontSize: 13, lineHeight: 1.6 }}
                  >
                    {busy === 'loading' ? t('common.loading') : t('style.pack.summaryCurrentEmpty')}
                  </div>
                ) : (
                  <div
                    className="ol-thinscroll"
                    style={{
                      overflow: 'auto',
                      padding: 18,
                      display: 'flex',
                      flexDirection: 'column',
                      gap: 16,
                    }}
                  >
                    <div
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'space-between',
                        gap: 12,
                        flexWrap: 'wrap',
                      }}
                    >
                      <div
                        style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}
                      >
                        <Pill tone={draft.kind === 'builtin' ? 'outline' : 'blue'}>
                          {draft.kind === 'builtin'
                            ? t('style.pack.builtin')
                            : t('style.pack.imported')}
                        </Pill>
                        <Pill tone={modeTone(draft.baseMode)}>
                          {t(`style.modes.${draft.baseMode}.name`)}
                        </Pill>
                        {draft.active && <Pill tone="dark">{t('style.pack.active')}</Pill>}
                        {dirty && <Pill tone="outline">{t('style.pack.unsaved')}</Pill>}
                      </div>
                      <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                        <Btn
                          variant="ghost"
                          icon="archive"
                          onClick={() => void handleExportZip()}
                          disabled={busy === 'exporting'}
                        >
                          {t('style.pack.exportZip')}
                        </Btn>
                        <span
                          title={
                            draft?.kind === 'builtin'
                              ? t('style.pack.publishBuiltinRejected')
                              : !canPublish
                                ? t('style.pack.publishDisabledHint')
                                : ''
                          }
                        >
                          <Btn
                            variant="ghost"
                            icon="cloud"
                            onClick={() => void handlePublishToMarketplace()}
                            disabled={
                              !canPublish || draft?.kind === 'builtin' || busy === 'exporting'
                            }
                          >
                            {draft?.originPackId
                              ? t('style.pack.updateMarketplace')
                              : t('style.pack.publishMarketplace')}
                          </Btn>
                        </span>
                        <Btn
                          variant={draft.active ? 'soft' : 'blue'}
                          icon="check"
                          disabled={draft.active || busy === 'activating'}
                          onClick={() => void handleActivate(draft)}
                        >
                          {draft.active ? t('style.pack.active') : t('style.pack.activate')}
                        </Btn>
                      </div>
                    </div>

                    <div
                      className="ol-grid-12 ol-grid-fields"
                      data-columns={stackLayout ? '1' : undefined}
                    >
                      <label style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                        <span
                          style={{
                            fontSize: 12,
                            fontWeight: 'var(--ol-font-weight-semibold)',
                            color: 'var(--ol-ink)',
                          }}
                        >
                          {t('style.pack.fieldName')}
                        </span>
                        <input
                          value={draft.name}
                          onChange={(event) => patchDraft({ name: event.target.value })}
                          style={inputStyle}
                        />
                      </label>
                      <label style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                        <span
                          style={{
                            fontSize: 12,
                            fontWeight: 'var(--ol-font-weight-semibold)',
                            color: 'var(--ol-ink)',
                          }}
                        >
                          {t('style.pack.fieldAuthor')}
                        </span>
                        <input
                          value={draft.author ?? ''}
                          onChange={(event) => patchDraft({ author: event.target.value || null })}
                          style={inputStyle}
                          placeholder={t('style.pack.fieldAuthorPlaceholder')}
                        />
                      </label>
                      <label style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                        <span
                          style={{
                            fontSize: 12,
                            fontWeight: 'var(--ol-font-weight-semibold)',
                            color: 'var(--ol-ink)',
                          }}
                        >
                          {t('style.pack.fieldVersion')}
                        </span>
                        <input
                          value={draft.version}
                          onChange={(event) => patchDraft({ version: event.target.value })}
                          style={inputStyle}
                        />
                      </label>
                      <label style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                        <span
                          style={{
                            fontSize: 12,
                            fontWeight: 'var(--ol-font-weight-semibold)',
                            color: 'var(--ol-ink)',
                          }}
                        >
                          {t('style.pack.fieldTags')}
                        </span>
                        <input
                          value={draft.tags.join(', ')}
                          onChange={(event) =>
                            patchDraft({
                              tags: event.target.value
                                .split(',')
                                .map((value) => value.trim())
                                .filter(Boolean),
                            })
                          }
                          style={inputStyle}
                          placeholder={t('style.pack.fieldTagsPlaceholder')}
                        />
                      </label>
                    </div>

                    <label style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                      <span
                        style={{
                          fontSize: 12,
                          fontWeight: 'var(--ol-font-weight-semibold)',
                          color: 'var(--ol-ink)',
                        }}
                      >
                        {t('style.pack.fieldDescription')}
                      </span>
                      <textarea
                        value={draft.description}
                        onChange={(event) => patchDraft({ description: event.target.value })}
                        style={{ ...textareaStyle, minHeight: 86 }}
                      />
                    </label>

                    <div
                      className="ol-grid-12 ol-grid-fields"
                      data-columns={stackLayout ? '1' : undefined}
                    >
                      <label style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                        <span
                          style={{
                            fontSize: 12,
                            fontWeight: 'var(--ol-font-weight-semibold)',
                            color: 'var(--ol-ink)',
                          }}
                        >
                          {t('style.pack.fieldModel')}
                        </span>
                        <input
                          value={draft.recommendedModel ?? ''}
                          onChange={(event) =>
                            patchDraft({ recommendedModel: event.target.value || null })
                          }
                          style={inputStyle}
                          placeholder={t('style.pack.fieldModelPlaceholder')}
                        />
                        <span
                          style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.55 }}
                        >
                          {t('style.pack.fieldModelHint')}
                        </span>
                      </label>
                      <label style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                        <span
                          style={{
                            fontSize: 12,
                            fontWeight: 'var(--ol-font-weight-semibold)',
                            color: 'var(--ol-ink)',
                          }}
                        >
                          {t('style.pack.fieldCompatibility')}
                        </span>
                        <input
                          value={draft.compatibleAppVersion ?? ''}
                          onChange={(event) =>
                            patchDraft({ compatibleAppVersion: event.target.value || null })
                          }
                          style={inputStyle}
                          placeholder={t('style.pack.fieldCompatibilityPlaceholder')}
                        />
                      </label>
                    </div>

                    {workflowView === 'dictation'
                      ? dictationPromptEditor
                      : selectionWorkflowEditors}

                    {workflowView === 'dictation' && (
                      <Card
                        padding={16}
                        style={{
                          background: 'var(--ol-style-subtle-bg)',
                          border: '0.5px solid var(--ol-line)',
                        }}
                      >
                        <div
                          style={{
                            display: 'flex',
                            alignItems: 'flex-start',
                            justifyContent: 'space-between',
                            gap: 12,
                            flexWrap: 'wrap',
                            marginBottom: 12,
                          }}
                        >
                          <div>
                            <div
                              style={{
                                fontSize: 13,
                                fontWeight: 'var(--ol-font-weight-semibold)',
                                color: 'var(--ol-ink)',
                              }}
                            >
                              {t('style.pack.runtimeTitle')}
                            </div>
                            <div
                              style={{
                                fontSize: 11.5,
                                color: 'var(--ol-ink-4)',
                                marginTop: 4,
                                lineHeight: 1.6,
                              }}
                            >
                              {t('style.pack.runtimeDesc')}
                            </div>
                          </div>
                        </div>

                        <div style={{ display: 'grid', gap: 8, marginBottom: 8 }}>
                          <DirectiveRow
                            title={t('style.pack.runtimeContextTitle')}
                            detail={t('style.pack.runtimeContextDesc')}
                            active={Boolean(runtimePreview?.contextPremise)}
                            activeLabel={t('style.pack.runtimeActive')}
                            inactiveLabel={t('style.pack.runtimeInactive')}
                            inactiveHint={t('style.pack.runtimeContextEmpty')}
                          />
                          <DirectiveRow
                            title={t('style.pack.runtimeHotwordTitle')}
                            detail={t('style.pack.runtimeHotwordDesc')}
                            active={Boolean(runtimePreview?.hotwordBlock)}
                            activeLabel={t('style.pack.runtimeActive')}
                            inactiveLabel={t('style.pack.runtimeInactive')}
                            inactiveHint={t('style.pack.runtimeHotwordEmpty')}
                          />
                          <DirectiveRow
                            title={t('style.pack.runtimeHistoryTitle')}
                            detail={t('style.pack.runtimeHistoryDesc')}
                            active={Boolean(runtimePreview?.historyInstruction)}
                            activeLabel={t('style.pack.runtimeActive')}
                            inactiveLabel={t('style.pack.runtimeInactive')}
                            inactiveHint={t('style.pack.runtimeHistoryEmpty')}
                          />
                        </div>
                        <div
                          style={{
                            fontSize: 11.5,
                            color: runtimePreviewError
                              ? 'var(--ol-red, #b91c1c)'
                              : 'var(--ol-ink-4)',
                            marginTop: 10,
                            lineHeight: 1.55,
                          }}
                        >
                          {runtimePreviewError
                            ? t('style.pack.runtimePreviewFailed', { err: runtimePreviewError })
                            : t('style.pack.runtimePreviewOmittedFrontApp')}
                        </div>
                      </Card>
                    )}

                    <div
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'space-between',
                        gap: 12,
                        flexWrap: 'wrap',
                      }}
                    >
                      <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                        <Btn
                          variant={dirty ? 'blue' : 'ghost'}
                          icon="check"
                          onClick={() => void handleSave()}
                          disabled={!dirty || busy === 'saving'}
                        >
                          {busy === 'saving' ? t('common.saving') : t('style.pack.save')}
                        </Btn>
                        <Btn
                          variant="ghost"
                          icon="refresh"
                          onClick={discardDraftChanges}
                          disabled={!dirty}
                        >
                          {t('style.pack.revert')}
                        </Btn>
                      </div>
                      <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                        {draft.kind === 'builtin' ? (
                          <Btn
                            variant="soft"
                            icon="refresh"
                            onClick={() => void handleResetBuiltin()}
                            disabled={busy === 'resetting'}
                          >
                            {t('style.pack.resetBuiltin')}
                          </Btn>
                        ) : (
                          <Btn
                            variant="soft"
                            icon="trash"
                            onClick={() => void handleDeleteImported()}
                            disabled={busy === 'deleting'}
                          >
                            {t('style.pack.deleteImported')}
                          </Btn>
                        )}
                      </div>
                    </div>

                    <div
                      style={{
                        padding: 14,
                        borderRadius: 'var(--ol-r-lg)',
                        background: 'var(--ol-style-subtle-bg)',
                        border: '0.5px solid var(--ol-line)',
                      }}
                    >
                      <div
                        style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 10 }}
                      >
                        <div
                          style={{
                            fontSize: 13,
                            fontWeight: 'var(--ol-font-weight-semibold)',
                            color: 'var(--ol-ink)',
                          }}
                        >
                          {t('style.pack.metaTitle')}
                        </div>
                        <Pill tone="default" size="sm">
                          {draft.id}
                        </Pill>
                      </div>
                      <div
                        className="ol-grid-12 ol-grid-meta"
                        data-columns={stackLayout ? '1' : undefined}
                      >
                        <MetaItem
                          label={t('style.pack.metaSource')}
                          value={
                            draft.kind === 'builtin'
                              ? t('style.pack.builtin')
                              : t('style.pack.imported')
                          }
                        />
                        <MetaItem
                          label={t('style.pack.metaBaseMode')}
                          value={t(`style.modes.${draft.baseMode}.name`)}
                        />
                        <MetaItem
                          label={t('style.pack.metaUpdatedAt')}
                          value={draft.updatedAt || '—'}
                        />
                      </div>
                    </div>

                    <div
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'space-between',
                        gap: 12,
                        flexWrap: 'wrap',
                      }}
                    >
                      <div>
                        <div
                          style={{
                            fontSize: 13,
                            fontWeight: 'var(--ol-font-weight-semibold)',
                            color: 'var(--ol-ink)',
                          }}
                        >
                          {t('style.pack.examplesTitle')}
                        </div>
                        <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 4 }}>
                          {t('style.pack.examplesDesc')}
                        </div>
                      </div>
                      <Btn variant="ghost" icon="plus" onClick={appendExample}>
                        {t('style.pack.addExample')}
                      </Btn>
                    </div>

                    <div style={{ display: 'grid', gap: 12 }}>
                      {draft.examples.length === 0 && (
                        <Card padding={18} style={{ background: 'var(--ol-surface-2)' }}>
                          <div
                            style={{ fontSize: 12.5, color: 'var(--ol-ink-3)', lineHeight: 1.6 }}
                          >
                            {t('style.pack.examplesEmpty')}
                          </div>
                        </Card>
                      )}

                      {draft.examples.map((example, index) => (
                        <Card
                          key={`${draft.id}-example-${index}`}
                          padding={16}
                          style={{
                            background: 'var(--ol-style-editor-bg)',
                          }}
                        >
                          <div
                            style={{
                              display: 'flex',
                              alignItems: 'center',
                              justifyContent: 'space-between',
                              gap: 12,
                              marginBottom: 12,
                            }}
                          >
                            <input
                              value={example.title ?? ''}
                              onChange={(event) =>
                                patchExample(index, { title: event.target.value })
                              }
                              style={{
                                ...inputStyle,
                                fontWeight: 'var(--ol-font-weight-semibold)',
                              }}
                              placeholder={t('style.pack.exampleTitlePlaceholder', {
                                index: index + 1,
                              })}
                            />
                            <button
                              type="button"
                              onClick={() => removeExample(index)}
                              aria-label={t('common.delete')}
                              style={{
                                width: 32,
                                height: 32,
                                flexShrink: 0,
                                border: '0.5px solid var(--ol-line-strong)',
                                borderRadius: 'var(--ol-control-radius)',
                                background: 'transparent',
                                color: 'var(--ol-ink-2)',
                                display: 'inline-flex',
                                alignItems: 'center',
                                justifyContent: 'center',
                                cursor: 'pointer',
                                transition:
                                  'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)',
                              }}
                            >
                              <Icon name="trash" size={15} />
                            </button>
                          </div>

                          <div
                            className="ol-grid-12 ol-grid-fields"
                            data-columns={stackLayout ? '1' : undefined}
                          >
                            <div
                              style={{
                                borderRadius: 'var(--ol-r-lg)',
                                border: '0.5px solid var(--ol-line)',
                                background: 'var(--ol-style-subtle-bg)',
                                padding: 14,
                              }}
                            >
                              <div
                                style={{
                                  display: 'flex',
                                  alignItems: 'center',
                                  gap: 8,
                                  marginBottom: 10,
                                }}
                              >
                                <Pill tone="outline" size="sm">
                                  {t('style.pack.exampleInput')}
                                </Pill>
                              </div>
                              <textarea
                                value={example.input}
                                onChange={(event) =>
                                  patchExample(index, { input: event.target.value })
                                }
                                style={{
                                  ...textareaStyle,
                                  minHeight: 120,
                                  background: 'var(--ol-style-input-bg)',
                                }}
                              />
                            </div>

                            <div
                              style={{
                                borderRadius: 'var(--ol-r-lg)',
                                border: '0.5px solid rgba(37,99,235,0.16)',
                                background: 'var(--ol-style-subtle-bg)',
                                padding: 14,
                              }}
                            >
                              <div
                                style={{
                                  display: 'flex',
                                  alignItems: 'center',
                                  gap: 8,
                                  marginBottom: 10,
                                }}
                              >
                                <Pill tone="blue" size="sm">
                                  {t('style.pack.exampleOutput')}
                                </Pill>
                              </div>
                              <textarea
                                value={example.output}
                                onChange={(event) =>
                                  patchExample(index, { output: event.target.value })
                                }
                                style={{
                                  ...textareaStyle,
                                  minHeight: 120,
                                  background: 'var(--ol-style-input-bg)',
                                }}
                              />
                            </div>
                          </div>
                        </Card>
                      ))}
                    </div>
                  </div>
                )}
              </Card>
            </motion.div>
          </>
        )}
      </AnimatePresence>
    </>
  );
}

function MetaItem({ label, value }: { label: string; value: string }) {
  return (
    <div
      style={{
        borderRadius: 'var(--ol-r-lg)',
        border: '0.5px solid var(--ol-line)',
        background: 'var(--ol-style-input-bg)',
        padding: '10px 12px',
      }}
    >
      <div
        style={{
          fontSize: 11,
          textTransform: 'uppercase',
          letterSpacing: '.08em',
          color: 'var(--ol-ink-4)',
          marginBottom: 6,
        }}
      >
        {label}
      </div>
      <div
        style={{
          fontSize: 12.5,
          lineHeight: 1.5,
          color: 'var(--ol-ink-2)',
          wordBreak: 'break-word',
        }}
      >
        {value}
      </div>
    </div>
  );
}

function DirectiveRow({
  title,
  detail,
  active,
  activeLabel,
  inactiveLabel,
  inactiveHint,
}: {
  title: string;
  detail: string;
  active: boolean;
  activeLabel: string;
  inactiveLabel: string;
  inactiveHint: string;
}) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        gap: 12,
        padding: '10px 12px',
        borderRadius: 'var(--ol-r-lg)',
        border: '0.5px solid var(--ol-line)',
        background: 'var(--ol-style-input-bg)',
      }}
    >
      <div style={{ minWidth: 0 }}>
        <div
          style={{
            fontSize: 12.5,
            fontWeight: 'var(--ol-font-weight-semibold)',
            color: 'var(--ol-ink)',
          }}
        >
          {title}
        </div>
        <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.5, marginTop: 2 }}>
          {active ? detail : inactiveHint}
        </div>
      </div>
      <Pill tone={active ? 'blue' : 'outline'} size="sm">
        {active ? activeLabel : inactiveLabel}
      </Pill>
    </div>
  );
}

const inputStyle: CSSProperties = {
  width: '100%',
  boxSizing: 'border-box',
  minHeight: 38,
  padding: '9px 11px',
  borderRadius: 'var(--ol-r-md)',
  border: '0.5px solid var(--ol-line-strong)',
  background: 'var(--ol-style-input-bg)',
  color: 'var(--ol-ink)',
  font: 'inherit',
  fontSize: 12.5,
};

const textareaStyle: CSSProperties = {
  width: '100%',
  boxSizing: 'border-box',
  padding: '11px 12px',
  borderRadius: 'var(--ol-r-lg)',
  border: '0.5px solid var(--ol-line-strong)',
  background: 'var(--ol-style-input-bg)',
  color: 'var(--ol-ink)',
  font: 'inherit',
  fontSize: 12.5,
  lineHeight: 1.65,
  resize: 'vertical',
};
