import { type CSSProperties, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  deleteStylePack,
  exportStylePackToZip,
  importStylePackFromZip,
  isTauri,
  listStylePacks,
  previewStylePackRuntime,
  resetBuiltinStylePack,
  saveStylePack,
  setActiveStylePack,
  setStylePackEnabled,
} from '../lib/ipc';
import type { PolishMode, StylePack, StylePackExample, StylePackRuntimeDiagnostics } from '../lib/types';
import { Btn, Card, PageHeader, Pill } from './_atoms';
import { Icon } from '../components/Icon';

type BusyAction =
  | 'loading'
  | 'saving'
  | 'importing'
  | 'exporting'
  | 'activating'
  | 'toggling'
  | 'resetting'
  | 'deleting'
  | null;

function clonePack(pack: StylePack): StylePack {
  return {
    ...pack,
    tags: [...pack.tags],
    examples: pack.examples.map(example => ({ ...example })),
  };
}

function editableFingerprint(pack: StylePack | null): string {
  if (!pack) return '';
  return JSON.stringify({
    name: pack.name,
    description: pack.description,
    author: pack.author ?? '',
    version: pack.version,
    prompt: pack.prompt,
    examples: pack.examples,
    tags: pack.tags,
    recommendedModel: pack.recommendedModel ?? '',
    compatibleAppVersion: pack.compatibleAppVersion ?? '',
  });
}

function blankExample(): StylePackExample {
  return {
    title: '',
    input: '',
    output: '',
  };
}

function modeTone(mode: PolishMode): 'default' | 'blue' | 'ok' | 'outline' | 'dark' {
  if (mode === 'raw') return 'outline';
  if (mode === 'light') return 'blue';
  if (mode === 'structured') return 'ok';
  return 'dark';
}

function sanitizeZipFileName(name: string) {
  const trimmed = name.trim() || 'style-pack';
  return trimmed.replace(/[<>:"/\\|?*]+/g, '-').replace(/\s+/g, '-').toLowerCase();
}

export function Style() {
  const { t } = useTranslation();
  const tp = (key: string, options?: Record<string, unknown>) => t(`style.packs.${key}`, options);

  const displayPackName = (pack: StylePack) => (
    pack.kind === 'builtin' ? t(`style.modes.${pack.baseMode}.name`) : pack.name
  );

  const displayPackDescription = (pack: StylePack) => (
    pack.kind === 'builtin' ? t(`style.modes.${pack.baseMode}.desc`) : pack.description
  );

  const copy = {
    kicker: tp('kicker'),
    title: tp('title'),
    desc: tp('desc'),
    loadFailed: (message: string) => tp('loadFailed', { message }),
    importZip: tp('importZip'),
    exportZip: tp('exportZip'),
    exportShort: tp('exportShort'),
    builtin: tp('builtin'),
    imported: tp('imported'),
    active: tp('active'),
    enabled: tp('enabled'),
    disabled: tp('disabled'),
    activate: tp('activate'),
    enable: tp('enable'),
    disable: tp('disable'),
    edit: tp('edit'),
    closeEditor: tp('closeEditor'),
    unsaved: tp('unsaved'),
    listTitle: tp('listTitle'),
    listDesc: tp('listDesc'),
    listCount: (count: number) => tp('listCount', { count }),
    save: tp('save'),
    revert: tp('revert'),
    saveSuccess: tp('saveSuccess'),
    saveFailed: (message: string) => tp('saveFailed', { message }),
    activateSuccess: (name: string) => tp('activateSuccess', { name }),
    activateFailed: (message: string) => tp('activateFailed', { message }),
    enableSuccess: (name: string) => tp('enableSuccess', { name }),
    disableSuccess: (name: string) => tp('disableSuccess', { name }),
    toggleFailed: (message: string) => tp('toggleFailed', { message }),
    importSuccess: (name: string) => tp('importSuccess', { name }),
    importFailed: (message: string) => tp('importFailed', { message }),
    exportSuccess: (path: string) => tp('exportSuccess', { path }),
    exportFailed: (message: string) => tp('exportFailed', { message }),
    exportDirtyFirst: tp('exportDirtyFirst'),
    resetBuiltin: tp('resetBuiltin'),
    resetSuccess: (name: string) => tp('resetSuccess', { name }),
    resetFailed: (message: string) => tp('resetFailed', { message }),
    deleteImported: tp('deleteImported'),
    deleteConfirm: (name: string) => tp('deleteConfirm', { name }),
    deleteSuccess: (name: string) => tp('deleteSuccess', { name }),
    deleteFailed: (message: string) => tp('deleteFailed', { message }),
    summaryBuiltin: tp('summaryBuiltin'),
    summaryBuiltinHint: tp('summaryBuiltinHint'),
    summaryImported: tp('summaryImported'),
    summaryImportedHint: tp('summaryImportedHint'),
    summaryEnabled: tp('summaryEnabled'),
    summaryCurrent: (name: string) => tp('summaryCurrent', { name }),
    summaryCurrentEmpty: tp('summaryCurrentEmpty'),
    editorTitle: tp('editorTitle'),
    editorDesc: tp('editorDesc'),
    metaTitle: tp('metaTitle'),
    metaSource: tp('metaSource'),
    metaBaseMode: tp('metaBaseMode'),
    metaStatus: tp('metaStatus'),
    metaUpdatedAt: tp('metaUpdatedAt'),
    fieldName: tp('fieldName'),
    fieldAuthor: tp('fieldAuthor'),
    fieldAuthorPlaceholder: tp('fieldAuthorPlaceholder'),
    fieldVersion: tp('fieldVersion'),
    fieldTags: tp('fieldTags'),
    fieldTagsPlaceholder: tp('fieldTagsPlaceholder'),
    fieldDescription: tp('fieldDescription'),
    fieldModel: tp('fieldModel'),
    fieldModelPlaceholder: tp('fieldModelPlaceholder'),
    fieldModelHint: tp('fieldModelHint'),
    fieldCompatibility: tp('fieldCompatibility'),
    fieldCompatibilityPlaceholder: tp('fieldCompatibilityPlaceholder'),
    fullPromptTitle: tp('fullPromptTitle'),
    fullPromptHint: tp('fullPromptHint'),
    runtimeTitle: tp('runtimeTitle'),
    runtimeDesc: tp('runtimeDesc'),
    runtimeDirectiveContextTitle: tp('runtimeDirectiveContextTitle'),
    runtimeDirectiveContextDesc: tp('runtimeDirectiveContextDesc'),
    runtimeDirectiveContextEmpty: tp('runtimeDirectiveContextEmpty'),
    runtimeDirectiveHotwordTitle: tp('runtimeDirectiveHotwordTitle'),
    runtimeDirectiveHotwordDesc: tp('runtimeDirectiveHotwordDesc'),
    runtimeDirectiveHotwordEmpty: tp('runtimeDirectiveHotwordEmpty'),
    runtimeDirectiveHistoryTitle: tp('runtimeDirectiveHistoryTitle'),
    runtimeDirectiveHistoryDesc: tp('runtimeDirectiveHistoryDesc'),
    runtimeDirectiveHistoryEmpty: tp('runtimeDirectiveHistoryEmpty'),
    runtimeDirectiveActive: tp('runtimeDirectiveActive'),
    runtimeDirectiveInactive: tp('runtimeDirectiveInactive'),
    runtimePreviewFailed: (message: string) => tp('runtimePreviewFailed', { message }),
    runtimePreviewOmittedFrontApp: tp('runtimePreviewOmittedFrontApp'),
    examplesTitle: tp('examplesTitle'),
    examplesDesc: tp('examplesDesc'),
    addExample: tp('addExample'),
    examplesEmpty: tp('examplesEmpty'),
    exampleTitlePlaceholder: (index: number) => tp('exampleTitlePlaceholder', { index }),
    exampleInput: tp('exampleInput'),
    exampleOutput: tp('exampleOutput'),
    examplesCount: (count: number) => tp('examplesCount', { count }),
    promptCharCount: (count: number) => tp('promptCharCount', { count }),
    discardCloseConfirm: tp('discardCloseConfirm'),
    discardSwitchConfirm: (name: string) => tp('discardSwitchConfirm', { name }),
  };

  const [packs, setPacks] = useState<StylePack[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<StylePack | null>(null);
  const [busy, setBusy] = useState<BusyAction>('loading');
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [editorOpen, setEditorOpen] = useState(false);
  const [runtimePreview, setRuntimePreview] = useState<StylePackRuntimeDiagnostics | null>(null);
  const [runtimePreviewError, setRuntimePreviewError] = useState<string | null>(null);

  const loadPacks = async (preferredId?: string | null) => {
    setBusy('loading');
    setError(null);
    try {
      const next = await listStylePacks();
      setPacks(next);
      const nextSelectedId =
        (preferredId && next.some(pack => pack.id === preferredId) && preferredId) ||
        next.find(pack => pack.active)?.id ||
        next[0]?.id ||
        null;
      setSelectedId(nextSelectedId);
    } catch (loadError) {
      setError(copy.loadFailed(String(loadError)));
    } finally {
      setBusy(null);
    }
  };

  useEffect(() => {
    void loadPacks();
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        unlisten = await listen('prefs:changed', () => {
          void loadPacks(selectedId);
        });
        if (cancelled && unlisten) unlisten();
      } catch {
        // Browser dev mock does not have the event bridge.
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [selectedId]);

  const selectedPack = packs.find(pack => pack.id === selectedId) ?? null;
  const activePack = packs.find(pack => pack.active) ?? null;
  const builtinCount = packs.filter(pack => pack.kind === 'builtin').length;
  const importedCount = packs.filter(pack => pack.kind === 'imported').length;
  const enabledCount = packs.filter(pack => pack.enabled).length;

  useEffect(() => {
    if (!selectedPack) {
      setDraft(null);
      return;
    }
    setDraft(clonePack(selectedPack));
  }, [selectedPack?.id, selectedPack?.updatedAt, selectedPack?.active, selectedPack?.enabled]);

  useEffect(() => {
    if (!editorOpen || !draft) {
      setRuntimePreview(null);
      setRuntimePreviewError(null);
      return;
    }
    const timer = window.setTimeout(() => {
      void previewStylePackRuntime(draft)
        .then(preview => {
          setRuntimePreview(preview);
          setRuntimePreviewError(null);
        })
        .catch(previewError => {
          setRuntimePreview(null);
          setRuntimePreviewError(String(previewError));
        });
    }, 140);
    return () => window.clearTimeout(timer);
  }, [editorOpen, draft]);

  const dirty = editableFingerprint(selectedPack) !== editableFingerprint(draft);

  const focusPack = (packId: string) => {
    setSelectedId(packId);
    setNotice(null);
    setError(null);
  };

  const discardDraftChanges = () => {
    if (selectedPack) {
      setDraft(clonePack(selectedPack));
    }
  };

  const closeEditor = () => {
    if (dirty) {
      if (!window.confirm(copy.discardCloseConfirm)) {
        return;
      }
      discardDraftChanges();
    }
    setEditorOpen(false);
  };

  const openEditorForPack = (pack: StylePack) => {
    if (editorOpen && dirty && selectedPack && selectedPack.id !== pack.id) {
      if (!window.confirm(copy.discardSwitchConfirm(displayPackName(pack)))) {
        return;
      }
    }
    focusPack(pack.id);
    setEditorOpen(true);
  };

  useEffect(() => {
    if (!editorOpen) return;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        closeEditor();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => {
      document.body.style.overflow = previousOverflow;
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [editorOpen, dirty, selectedPack, draft]);

  const patchDraft = (patch: Partial<StylePack>) => {
    setDraft(current => (current ? { ...current, ...patch } : current));
  };

  const patchExample = (index: number, patch: Partial<StylePackExample>) => {
    setDraft(current => {
      if (!current) return current;
      const nextExamples = current.examples.map((example, currentIndex) =>
        currentIndex === index ? { ...example, ...patch } : example,
      );
      return { ...current, examples: nextExamples };
    });
  };

  const appendExample = () => {
    setDraft(current => (current ? { ...current, examples: [...current.examples, blankExample()] } : current));
  };

  const removeExample = (index: number) => {
    setDraft(current => {
      if (!current) return current;
      return {
        ...current,
        examples: current.examples.filter((_, currentIndex) => currentIndex !== index),
      };
    });
  };

  const showSuccess = (message: string) => {
    setNotice(message);
    setError(null);
  };

  const handleSave = async () => {
    if (!draft) return;
    setBusy('saving');
    try {
      const saved = await saveStylePack({
        ...draft,
        tags: draft.tags.filter(Boolean),
      });
      showSuccess(copy.saveSuccess);
      await loadPacks(saved.id);
    } catch (saveError) {
      setError(copy.saveFailed(String(saveError)));
    } finally {
      setBusy(null);
    }
  };

  const handleActivate = async (pack: StylePack) => {
    setBusy('activating');
    try {
      await setActiveStylePack(pack.id);
      showSuccess(copy.activateSuccess(displayPackName(pack)));
      await loadPacks(pack.id);
    } catch (activateError) {
      setError(copy.activateFailed(String(activateError)));
    } finally {
      setBusy(null);
    }
  };

  const handleToggleEnabled = async (pack: StylePack) => {
    setBusy('toggling');
    try {
      await setStylePackEnabled(pack.id, !pack.enabled);
      showSuccess(pack.enabled ? copy.disableSuccess(displayPackName(pack)) : copy.enableSuccess(displayPackName(pack)));
      await loadPacks(pack.id);
    } catch (toggleError) {
      setError(copy.toggleFailed(String(toggleError)));
    } finally {
      setBusy(null);
    }
  };

  const handleResetBuiltin = async () => {
    if (!selectedPack || selectedPack.kind !== 'builtin') return;
    setBusy('resetting');
    try {
      await resetBuiltinStylePack(selectedPack.id);
      showSuccess(copy.resetSuccess(displayPackName(selectedPack)));
      await loadPacks(selectedPack.id);
    } catch (resetError) {
      setError(copy.resetFailed(String(resetError)));
    } finally {
      setBusy(null);
    }
  };

  const handleDeleteImported = async () => {
    if (!selectedPack || selectedPack.kind !== 'imported') return;
    if (!window.confirm(copy.deleteConfirm(displayPackName(selectedPack)))) {
      return;
    }
    setBusy('deleting');
    try {
      await deleteStylePack(selectedPack.id);
      showSuccess(copy.deleteSuccess(displayPackName(selectedPack)));
      setEditorOpen(false);
      await loadPacks();
    } catch (deleteError) {
      setError(copy.deleteFailed(String(deleteError)));
    } finally {
      setBusy(null);
    }
  };

  const handleImportZip = async () => {
    setBusy('importing');
    try {
      let zipPath: string | null = null;
      if (isTauri) {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const picked = await open({
          filters: [{ name: 'Style Pack ZIP', extensions: ['zip'] }],
          multiple: false,
        });
        zipPath = typeof picked === 'string' ? picked : null;
      } else {
        zipPath = 'mock-style-pack.zip';
      }
      if (!zipPath) {
        setBusy(null);
        return;
      }
      const imported = await importStylePackFromZip(zipPath);
      showSuccess(copy.importSuccess(imported.name));
      await loadPacks(imported.id);
    } catch (importError) {
      setError(copy.importFailed(String(importError)));
    } finally {
      setBusy(null);
    }
  };

  const handleExportZip = async (pack = selectedPack) => {
    if (!pack) return;
    if (editorOpen && dirty && selectedPack && pack.id === selectedPack.id) {
      setError(copy.exportDirtyFirst);
      setNotice(null);
      return;
    }
    setBusy('exporting');
    try {
      const defaultName = `${sanitizeZipFileName(displayPackName(pack))}.zip`;
      let targetPath: string | null = null;
      if (isTauri) {
        const { save } = await import('@tauri-apps/plugin-dialog');
        targetPath = await save({
          defaultPath: defaultName,
          filters: [{ name: 'Style Pack ZIP', extensions: ['zip'] }],
        });
      } else {
        targetPath = `~/Downloads/${defaultName}`;
      }
      if (!targetPath) {
        setBusy(null);
        return;
      }
      const savedPath = await exportStylePackToZip(pack.id, targetPath);
      showSuccess(copy.exportSuccess(savedPath));
    } catch (exportError) {
      setError(copy.exportFailed(String(exportError)));
    } finally {
      setBusy(null);
    }
  };

  return (
    <>
      <PageHeader
        kicker={copy.kicker}
        title={copy.title}
        desc={copy.desc}
        right={(
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap', justifyContent: 'flex-end' }}>
            <Btn variant="ghost" icon="refresh" onClick={() => void loadPacks(selectedId)} disabled={busy === 'loading'}>
              {t('common.refresh')}
            </Btn>
            <Btn variant="blue" icon="archive" onClick={() => void handleImportZip()} disabled={busy === 'importing'}>
              {busy === 'importing' ? t('common.loading') : copy.importZip}
            </Btn>
          </div>
        )}
      />

      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))', gap: 12, marginBottom: 16 }}>
        <Card padding={16} glassy>
          <div style={{ fontSize: 11, letterSpacing: '.08em', textTransform: 'uppercase', color: 'var(--ol-ink-4)', marginBottom: 8 }}>
            {copy.summaryBuiltin}
          </div>
          <div style={{ fontSize: 24, fontWeight: 600, color: 'var(--ol-ink)' }}>{builtinCount}</div>
          <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', marginTop: 4 }}>{copy.summaryBuiltinHint}</div>
        </Card>
        <Card padding={16} glassy>
          <div style={{ fontSize: 11, letterSpacing: '.08em', textTransform: 'uppercase', color: 'var(--ol-ink-4)', marginBottom: 8 }}>
            {copy.summaryImported}
          </div>
          <div style={{ fontSize: 24, fontWeight: 600, color: 'var(--ol-ink)' }}>{importedCount}</div>
          <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', marginTop: 4 }}>{copy.summaryImportedHint}</div>
        </Card>
        <Card padding={16} glassy>
          <div style={{ fontSize: 11, letterSpacing: '.08em', textTransform: 'uppercase', color: 'var(--ol-ink-4)', marginBottom: 8 }}>
            {copy.summaryEnabled}
          </div>
          <div style={{ fontSize: 24, fontWeight: 600, color: 'var(--ol-ink)' }}>{enabledCount}</div>
          <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', marginTop: 4 }}>
            {activePack ? copy.summaryCurrent(displayPackName(activePack)) : copy.summaryCurrentEmpty}
          </div>
        </Card>
      </div>

      {(notice || error) && (
        <div
          role={error ? 'alert' : 'status'}
          style={{
            marginBottom: 14,
            padding: '12px 14px',
            borderRadius: 12,
            border: error ? '0.5px solid rgba(239,68,68,0.22)' : '0.5px solid rgba(37,99,235,0.16)',
            background: error ? 'rgba(254,242,242,0.9)' : 'rgba(239,246,255,0.92)',
            color: error ? 'var(--ol-red, #b91c1c)' : 'var(--ol-blue)',
            fontSize: 12.5,
            lineHeight: 1.55,
          }}
        >
          {error ?? notice}
        </div>
      )}

      <Card padding={0} style={{ overflow: 'hidden' }}>
        <div style={{ padding: 18, borderBottom: '0.5px solid var(--ol-line)' }}>
          <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
            <div>
              <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--ol-ink)' }}>{copy.listTitle}</div>
              <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', marginTop: 4, maxWidth: 760 }}>{copy.listDesc}</div>
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap', justifyContent: 'flex-end' }}>
              <Pill tone="outline">{copy.listCount(packs.length)}</Pill>
            </div>
          </div>
        </div>
        <div style={{ padding: 18 }}>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(260px, 1fr))', gap: 12 }}>
            {packs.map(pack => {
              const selected = pack.id === selectedId;
              return (
                <div
                  key={pack.id}
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    textAlign: 'left',
                    border: '0.5px solid',
                    borderColor: selected || pack.active ? 'var(--ol-blue)' : 'var(--ol-line)',
                    background: pack.active
                      ? 'linear-gradient(180deg, rgba(239,246,255,0.92), rgba(255,255,255,0.98))'
                      : 'linear-gradient(180deg, rgba(255,255,255,0.98), rgba(248,250,252,0.92))',
                    borderRadius: 18,
                    padding: 16,
                    boxShadow: selected || pack.active ? '0 0 0 3px var(--ol-blue-ring)' : 'none',
                    cursor: 'default',
                    opacity: pack.enabled ? 1 : 0.72,
                    minHeight: 204,
                    transition: 'border-color 0.16s var(--ol-motion-quick), box-shadow 0.18s var(--ol-motion-soft), opacity 0.18s var(--ol-motion-soft)',
                  }}
                >
                  <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 10, marginBottom: 10 }}>
                    <div style={{ minWidth: 0 }}>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                        <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--ol-ink)' }}>{displayPackName(pack)}</div>
                        <Pill tone={pack.kind === 'builtin' ? 'outline' : 'blue'} size="sm">
                          {pack.kind === 'builtin' ? copy.builtin : copy.imported}
                        </Pill>
                        <div style={{ minWidth: 42, minHeight: 24, display: 'flex', alignItems: 'center' }}>
                          {pack.active ? (
                            <Pill tone="dark" size="sm">{copy.active}</Pill>
                          ) : !pack.enabled ? (
                            <Pill tone="default" size="sm">{copy.disabled}</Pill>
                          ) : null}
                        </div>
                      </div>
                      <div
                        style={{
                          fontSize: 12.5,
                          color: 'var(--ol-ink-3)',
                          lineHeight: 1.6,
                          display: '-webkit-box',
                          WebkitBoxOrient: 'vertical',
                          WebkitLineClamp: 3,
                          overflow: 'hidden',
                          marginTop: 8,
                          minHeight: 60,
                        }}
                      >
                        {displayPackDescription(pack)}
                      </div>
                    </div>
                    <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-end', gap: 8, flexShrink: 0 }}>
                      <div
                        style={{
                          width: 36,
                          height: 36,
                          borderRadius: 12,
                          display: 'grid',
                          placeItems: 'center',
                          background: pack.active ? 'rgba(37,99,235,0.12)' : 'rgba(15,23,42,0.05)',
                          color: pack.active ? 'var(--ol-blue)' : 'var(--ol-ink-3)',
                        }}
                      >
                        <Icon name={pack.kind === 'builtin' ? 'sparkle' : 'archive'} size={16} />
                      </div>
                      <Btn
                        size="sm"
                        variant={selected ? 'blue' : 'ghost'}
                        icon="expand"
                        onClick={() => openEditorForPack(pack)}
                      >
                        {copy.edit}
                      </Btn>
                    </div>
                  </div>

                  <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap', minHeight: 24, marginBottom: 12 }}>
                    <Pill tone={modeTone(pack.baseMode)} size="sm">{t(`style.modes.${pack.baseMode}.name`)}</Pill>
                    {pack.tags.slice(0, 1).map(tag => (
                      <Pill key={`${pack.id}-${tag}`} tone="default" size="sm">{tag}</Pill>
                    ))}
                  </div>

                  <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', marginTop: 'auto' }}>
                    <Btn
                      size="sm"
                      variant={pack.active ? 'soft' : 'ghost'}
                      disabled={pack.active || busy === 'activating'}
                      onClick={() => void handleActivate(pack)}
                    >
                      {pack.active ? copy.active : copy.activate}
                    </Btn>
                    <Btn
                      size="sm"
                      variant="ghost"
                      disabled={busy === 'toggling'}
                      onClick={() => void handleToggleEnabled(pack)}
                    >
                      {pack.enabled ? copy.disable : copy.enable}
                    </Btn>
                    <Btn
                      size="sm"
                      variant="ghost"
                      icon="archive"
                      disabled={busy === 'exporting'}
                      onClick={() => void handleExportZip(pack)}
                    >
                      {copy.exportShort}
                    </Btn>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      </Card>

      {editorOpen && (
        <>
          <div
            aria-hidden="true"
            onClick={closeEditor}
            style={{
              position: 'fixed',
              inset: 0,
              background: 'rgba(15,23,42,0.24)',
              backdropFilter: 'blur(6px)',
              WebkitBackdropFilter: 'blur(6px)',
              zIndex: 40,
            }}
          />
          <div
            role="dialog"
            aria-modal="true"
            aria-label={copy.editorTitle}
            style={{
              position: 'fixed',
              top: 16,
              right: 16,
              bottom: 16,
              width: 'min(760px, calc(100vw - 32px))',
              zIndex: 41,
            }}
          >
            <Card
              padding={0}
              style={{
                height: '100%',
                display: 'grid',
                gridTemplateRows: 'auto minmax(0, 1fr)',
                overflow: 'hidden',
                boxShadow: '0 24px 80px rgba(15,23,42,0.22)',
              }}
            >
              <div style={{ padding: 18, borderBottom: '0.5px solid var(--ol-line)' }}>
                <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 12 }}>
                  <div style={{ minWidth: 0 }}>
                    <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--ol-ink)' }}>{copy.editorTitle}</div>
                    <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', marginTop: 4, lineHeight: 1.6 }}>{copy.editorDesc}</div>
                  </div>
                  <button
                    type="button"
                    onClick={closeEditor}
                    aria-label={copy.closeEditor}
                    style={{
                      width: 34,
                      height: 34,
                      borderRadius: 10,
                      border: '0.5px solid var(--ol-line)',
                      background: 'var(--ol-surface-2)',
                      color: 'var(--ol-ink-3)',
                      display: 'grid',
                      placeItems: 'center',
                      flexShrink: 0,
                    }}
                  >
                    <Icon name="close" size={15} />
                  </button>
                </div>
              </div>

              {!draft ? (
                <div style={{ padding: 28, color: 'var(--ol-ink-3)', fontSize: 13, lineHeight: 1.6 }}>
                  {busy === 'loading' ? t('common.loading') : copy.summaryCurrentEmpty}
                </div>
              ) : (
                <div className="ol-thinscroll" style={{ overflow: 'auto', padding: 18, display: 'flex', flexDirection: 'column', gap: 16 }}>
                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                      <Pill tone={draft.kind === 'builtin' ? 'outline' : 'blue'}>
                        {draft.kind === 'builtin' ? copy.builtin : copy.imported}
                      </Pill>
                      <Pill tone={modeTone(draft.baseMode)}>{t(`style.modes.${draft.baseMode}.name`)}</Pill>
                      {draft.active && <Pill tone="dark">{copy.active}</Pill>}
                      {dirty && <Pill tone="outline">{copy.unsaved}</Pill>}
                    </div>
                    <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                      <Btn variant="ghost" icon="archive" onClick={() => void handleExportZip()} disabled={busy === 'exporting'}>
                        {copy.exportZip}
                      </Btn>
                      <Btn
                        variant="ghost"
                        disabled={busy === 'toggling'}
                        onClick={() => void handleToggleEnabled(draft)}
                      >
                        {draft.enabled ? copy.disable : copy.enable}
                      </Btn>
                      <Btn
                        variant={draft.active ? 'soft' : 'blue'}
                        icon="check"
                        disabled={draft.active || busy === 'activating'}
                        onClick={() => void handleActivate(draft)}
                      >
                        {draft.active ? copy.active : copy.activate}
                      </Btn>
                    </div>
                  </div>

                  <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))', gap: 12 }}>
                    <label style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                      <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink)' }}>{copy.fieldName}</span>
                      <input
                        value={draft.name}
                        onChange={event => patchDraft({ name: event.target.value })}
                        style={inputStyle}
                      />
                    </label>
                    <label style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                      <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink)' }}>{copy.fieldAuthor}</span>
                      <input
                        value={draft.author ?? ''}
                        onChange={event => patchDraft({ author: event.target.value || null })}
                        style={inputStyle}
                        placeholder={copy.fieldAuthorPlaceholder}
                      />
                    </label>
                    <label style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                      <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink)' }}>{copy.fieldVersion}</span>
                      <input
                        value={draft.version}
                        onChange={event => patchDraft({ version: event.target.value })}
                        style={inputStyle}
                      />
                    </label>
                    <label style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                      <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink)' }}>{copy.fieldTags}</span>
                      <input
                        value={draft.tags.join(', ')}
                        onChange={event => patchDraft({ tags: event.target.value.split(',').map(value => value.trim()).filter(Boolean) })}
                        style={inputStyle}
                        placeholder={copy.fieldTagsPlaceholder}
                      />
                    </label>
                  </div>

                  <label style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                    <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink)' }}>{copy.fieldDescription}</span>
                    <textarea
                      value={draft.description}
                      onChange={event => patchDraft({ description: event.target.value })}
                      style={{ ...textareaStyle, minHeight: 86 }}
                    />
                  </label>

                  <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))', gap: 12 }}>
                    <label style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                      <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink)' }}>{copy.fieldModel}</span>
                      <input
                        value={draft.recommendedModel ?? ''}
                        onChange={event => patchDraft({ recommendedModel: event.target.value || null })}
                        style={inputStyle}
                        placeholder={copy.fieldModelPlaceholder}
                      />
                      <span style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.55 }}>{copy.fieldModelHint}</span>
                    </label>
                    <label style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                      <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink)' }}>{copy.fieldCompatibility}</span>
                      <input
                        value={draft.compatibleAppVersion ?? ''}
                        onChange={event => patchDraft({ compatibleAppVersion: event.target.value || null })}
                        style={inputStyle}
                        placeholder={copy.fieldCompatibilityPlaceholder}
                      />
                    </label>
                  </div>

                  <label style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
                      <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink)' }}>{copy.fullPromptTitle}</span>
                      <Pill tone="default" size="sm">{copy.promptCharCount(draft.prompt.length)}</Pill>
                    </div>
                    <span style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.55 }}>{copy.fullPromptHint}</span>
                    <textarea
                      value={draft.prompt}
                      onChange={event => patchDraft({ prompt: event.target.value })}
                      style={{ ...textareaStyle, minHeight: 210 }}
                    />
                  </label>

                  <Card
                    padding={16}
                    style={{
                      background: 'linear-gradient(180deg, rgba(255,255,255,0.98), rgba(246,248,252,0.95))',
                      border: '0.5px solid rgba(148,163,184,0.24)',
                    }}
                  >
                    <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap', marginBottom: 12 }}>
                      <div>
                        <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--ol-ink)' }}>{copy.runtimeTitle}</div>
                        <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 4, lineHeight: 1.6 }}>{copy.runtimeDesc}</div>
                      </div>
                    </div>

                    <div style={{ display: 'grid', gap: 8, marginBottom: 8 }}>
                      <DirectiveRow
                        title={copy.runtimeDirectiveContextTitle}
                        detail={copy.runtimeDirectiveContextDesc}
                        active={Boolean(runtimePreview?.contextPremise)}
                        activeLabel={copy.runtimeDirectiveActive}
                        inactiveLabel={copy.runtimeDirectiveInactive}
                        inactiveHint={copy.runtimeDirectiveContextEmpty}
                      />
                      <DirectiveRow
                        title={copy.runtimeDirectiveHotwordTitle}
                        detail={copy.runtimeDirectiveHotwordDesc}
                        active={Boolean(runtimePreview?.hotwordBlock)}
                        activeLabel={copy.runtimeDirectiveActive}
                        inactiveLabel={copy.runtimeDirectiveInactive}
                        inactiveHint={copy.runtimeDirectiveHotwordEmpty}
                      />
                      <DirectiveRow
                        title={copy.runtimeDirectiveHistoryTitle}
                        detail={copy.runtimeDirectiveHistoryDesc}
                        active={Boolean(runtimePreview?.historyInstruction)}
                        activeLabel={copy.runtimeDirectiveActive}
                        inactiveLabel={copy.runtimeDirectiveInactive}
                        inactiveHint={copy.runtimeDirectiveHistoryEmpty}
                      />
                    </div>
                    <div style={{ fontSize: 11.5, color: runtimePreviewError ? 'var(--ol-red, #b91c1c)' : 'var(--ol-ink-4)', marginTop: 10, lineHeight: 1.55 }}>
                      {runtimePreviewError ? copy.runtimePreviewFailed(runtimePreviewError) : copy.runtimePreviewOmittedFrontApp}
                    </div>
                  </Card>

                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
                    <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                      <Btn variant={dirty ? 'blue' : 'ghost'} icon="check" onClick={() => void handleSave()} disabled={!dirty || busy === 'saving'}>
                        {busy === 'saving' ? t('common.saving') : copy.save}
                      </Btn>
                      <Btn variant="ghost" icon="refresh" onClick={discardDraftChanges} disabled={!dirty}>
                        {copy.revert}
                      </Btn>
                    </div>
                    <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                      {draft.kind === 'builtin' ? (
                        <Btn variant="soft" icon="refresh" onClick={() => void handleResetBuiltin()} disabled={busy === 'resetting'}>
                          {copy.resetBuiltin}
                        </Btn>
                      ) : (
                        <Btn variant="soft" icon="trash" onClick={() => void handleDeleteImported()} disabled={busy === 'deleting'}>
                          {copy.deleteImported}
                        </Btn>
                      )}
                    </div>
                  </div>

                  <div
                    style={{
                      padding: 14,
                      borderRadius: 14,
                      background: 'linear-gradient(180deg, rgba(248,250,252,0.98), rgba(241,245,249,0.95))',
                      border: '0.5px solid var(--ol-line)',
                    }}
                  >
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 10 }}>
                      <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--ol-ink)' }}>{copy.metaTitle}</div>
                      <Pill tone="default" size="sm">{draft.id}</Pill>
                    </div>
                    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(160px, 1fr))', gap: 10 }}>
                      <MetaItem label={copy.metaSource} value={draft.kind === 'builtin' ? copy.builtin : copy.imported} />
                      <MetaItem label={copy.metaBaseMode} value={t(`style.modes.${draft.baseMode}.name`)} />
                      <MetaItem label={copy.metaStatus} value={draft.enabled ? copy.enabled : copy.disabled} />
                      <MetaItem label={copy.metaUpdatedAt} value={draft.updatedAt || '—'} />
                    </div>
                  </div>

                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
                    <div>
                      <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--ol-ink)' }}>{copy.examplesTitle}</div>
                      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 4 }}>{copy.examplesDesc}</div>
                    </div>
                    <Btn variant="ghost" icon="plus" onClick={appendExample}>{copy.addExample}</Btn>
                  </div>

                  <div style={{ display: 'grid', gap: 12 }}>
                    {draft.examples.length === 0 && (
                      <Card padding={18} style={{ background: 'var(--ol-surface-2)' }}>
                        <div style={{ fontSize: 12.5, color: 'var(--ol-ink-3)', lineHeight: 1.6 }}>
                          {copy.examplesEmpty}
                        </div>
                      </Card>
                    )}

                    {draft.examples.map((example, index) => (
                      <Card
                        key={`${draft.id}-example-${index}`}
                        padding={16}
                        style={{
                          background: 'linear-gradient(180deg, rgba(255,255,255,0.98), rgba(248,250,252,0.98))',
                        }}
                      >
                        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, marginBottom: 12 }}>
                          <input
                            value={example.title ?? ''}
                            onChange={event => patchExample(index, { title: event.target.value })}
                            style={{ ...inputStyle, fontWeight: 600 }}
                            placeholder={copy.exampleTitlePlaceholder(index + 1)}
                          />
                          <Btn variant="ghost" size="sm" icon="trash" onClick={() => removeExample(index)}>
                            {t('common.delete')}
                          </Btn>
                        </div>

                        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(240px, 1fr))', gap: 12 }}>
                          <div
                            style={{
                              borderRadius: 14,
                              border: '0.5px solid rgba(148,163,184,0.22)',
                              background: 'rgba(248,250,252,0.9)',
                              padding: 14,
                            }}
                          >
                            <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 10 }}>
                              <Pill tone="outline" size="sm">{copy.exampleInput}</Pill>
                            </div>
                            <textarea
                              value={example.input}
                              onChange={event => patchExample(index, { input: event.target.value })}
                              style={{ ...textareaStyle, minHeight: 120, background: '#fff' }}
                            />
                          </div>

                          <div
                            style={{
                              borderRadius: 14,
                              border: '0.5px solid rgba(37,99,235,0.16)',
                              background: 'rgba(239,246,255,0.86)',
                              padding: 14,
                            }}
                          >
                            <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 10 }}>
                              <Pill tone="blue" size="sm">{copy.exampleOutput}</Pill>
                            </div>
                            <textarea
                              value={example.output}
                              onChange={event => patchExample(index, { output: event.target.value })}
                              style={{ ...textareaStyle, minHeight: 120, background: '#fff' }}
                            />
                          </div>
                        </div>
                      </Card>
                    ))}
                  </div>
                </div>
              )}
            </Card>
          </div>
        </>
      )}
    </>
  );
}

function MetaItem({ label, value }: { label: string; value: string }) {
  return (
    <div
      style={{
        borderRadius: 12,
        border: '0.5px solid rgba(148,163,184,0.2)',
        background: 'rgba(255,255,255,0.92)',
        padding: '10px 12px',
      }}
    >
      <div style={{ fontSize: 11, textTransform: 'uppercase', letterSpacing: '.08em', color: 'var(--ol-ink-4)', marginBottom: 6 }}>
        {label}
      </div>
      <div style={{ fontSize: 12.5, lineHeight: 1.5, color: 'var(--ol-ink-2)', wordBreak: 'break-word' }}>{value}</div>
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
        borderRadius: 12,
        border: '0.5px solid rgba(148,163,184,0.2)',
        background: 'rgba(255,255,255,0.92)',
      }}
    >
      <div style={{ minWidth: 0 }}>
        <div style={{ fontSize: 12.5, fontWeight: 600, color: 'var(--ol-ink)' }}>{title}</div>
        <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.5, marginTop: 2 }}>
          {active ? detail : inactiveHint}
        </div>
      </div>
      <Pill tone={active ? 'blue' : 'outline'} size="sm">{active ? activeLabel : inactiveLabel}</Pill>
    </div>
  );
}

const inputStyle: CSSProperties = {
  width: '100%',
  boxSizing: 'border-box',
  minHeight: 38,
  padding: '9px 11px',
  borderRadius: 10,
  border: '0.5px solid var(--ol-line-strong)',
  background: '#fff',
  color: 'var(--ol-ink)',
  font: 'inherit',
  fontSize: 12.5,
};

const textareaStyle: CSSProperties = {
  width: '100%',
  boxSizing: 'border-box',
  padding: '11px 12px',
  borderRadius: 12,
  border: '0.5px solid var(--ol-line-strong)',
  background: '#fff',
  color: 'var(--ol-ink)',
  font: 'inherit',
  fontSize: 12.5,
  lineHeight: 1.65,
  resize: 'vertical',
};
