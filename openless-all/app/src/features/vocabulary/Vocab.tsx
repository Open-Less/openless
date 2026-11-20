import { TextField } from '../../ui/controls/TextField';
import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import { ExpandingSearch } from '../../components/ExpandingSearch';
import { Icon } from '../../components/Icon';
import { Button as Btn } from '../../ui/controls/Button';
import { PageHeader } from '../../ui/layout/PageHeader';
import { Modal } from '../../ui/overlays/Modal';
import { useVocabularyWorkspace } from './useVocabularyWorkspace';
import { VocabularyWordCard } from './VocabularyWordCard';
import './vocabulary.css';

export function Vocab() {
  const {
    t,
    loading,
    error,
    presets,
    filter,
    setFilter,
    query,
    setQuery,
    wordAdded,
    setEditingEntry,
    editDraft,
    setEditDraft,
    editError,
    setEditError,
    setNewWordOpen,
    newWordDraft,
    setNewWordDraft,
    newWordTemplateIds,
    setNewWordTemplateIds,
    editMount,
    newWordMount,
    cardRefs,
    removingIds,
    selectedIds,
    setSelectedIds,
    selecting,
    setSelecting,
    batchBusy,
    removeEntries,
    toggleSelection,
    onToggle,
    openEdit,
    saveEdit,
    addNewWord,
    applyNewWordTemplates,
    sourceOf,
    learnedEntries,
    onRemoveAllLearnedEntries,
    needle,
    visibleEntries,
  } = useVocabularyWorkspace();
  const selectedCount = selectedIds.size;
  const visibleSelectedCount = visibleEntries.filter((entry) => selectedIds.has(entry.id)).length;
  const allVisibleSelected =
    visibleEntries.length > 0 && visibleSelectedCount === visibleEntries.length;
  // Reserve for every visible entry plus selections retained by other filters.
  const layoutCount = selectedCount - visibleSelectedCount + visibleEntries.length;
  const selectAllLabel = t('vocab.selectAllVisible');
  return (
    <div style={{ display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0 }}>
      <PageHeader kicker={t('vocab.kicker')} title={t('vocab.title')} desc={t('vocab.desc')} />

      <div className="ol-vocab-toolbar">
        <div className="ol-vocab-search-actions">
          <ExpandingSearch
            value={query}
            onChange={setQuery}
            placeholder={t('vocab.searchPlaceholder')}
            label={t('vocab.searchPlaceholder')}
            expandTo="right"
          />
          <div className="ol-vocab-word-actions">
            <button
              type="button"
              className="ol-field ol-field-icon-action ol-pressable"
              aria-label={t('vocab.newWord')}
              title={t('vocab.newWord')}
              onClick={() => {
                setNewWordOpen(true);
                setNewWordDraft('');
                setNewWordTemplateIds([]);
              }}
            >
              <Icon name="plus" size={15} />
            </button>
            <button
              type="button"
              className="ol-vocab-delete-selected"
              data-empty={selectedCount === 0 ? '' : undefined}
              disabled={batchBusy || selectedCount === 0}
              onClick={() => void removeEntries([...selectedIds])}
            >
              <span className="ol-vocab-action-content" aria-hidden="true">
                <Icon name="trash" size={13} />
                <span>{t('vocab.deleteSelected', { count: layoutCount })}</span>
              </span>
              <span className="ol-vocab-action-content">
                <Icon name="trash" size={13} />
                <span>{t('vocab.deleteSelected', { count: selectedCount })}</span>
              </span>
            </button>
          </div>
        </div>
        <div className="ol-vocab-selection-tools">
          <label className="ol-vocab-select-all" hidden={!selecting}>
            <input
              type="checkbox"
              disabled={batchBusy || visibleEntries.length === 0}
              checked={allVisibleSelected}
              ref={(element) => {
                if (element)
                  element.indeterminate = visibleSelectedCount > 0 && !allVisibleSelected;
              }}
              onChange={(event) => {
                const checked = event.target.checked;
                setSelectedIds((current) => {
                  const next = new Set(current);
                  visibleEntries.forEach((entry) =>
                    checked ? next.add(entry.id) : next.delete(entry.id),
                  );
                  return next;
                });
              }}
            />
            <span className="ol-vocab-reserved-label">
              <span aria-hidden="true">{selectAllLabel}</span>
              <span aria-hidden="true">{t('vocab.selectedCount', { count: layoutCount })}</span>
              <span>
                {selectedCount
                  ? t('vocab.selectedCount', { count: selectedCount })
                  : selectAllLabel}
              </span>
            </span>
          </label>
          <div className="ol-vocab-filter-actions">
            <div className="ol-seg ol-vocab-filters" role="tablist" aria-label={t('vocab.title')}>
              {(
                [
                  { id: 'all', icon: null },
                  { id: 'auto', icon: 'sparkle' },
                  { id: 'manual', icon: 'feather' },
                ] as const
              ).map((seg) => (
                <button
                  key={seg.id}
                  type="button"
                  role="tab"
                  aria-selected={filter === seg.id}
                  className={filter === seg.id ? 'ol-seg-item ol-seg-item-active' : 'ol-seg-item'}
                  onClick={() => setFilter(seg.id)}
                >
                  {seg.icon && <Icon name={seg.icon} size={13} />}
                  {t(`vocab.filter.${seg.id}`)}
                </button>
              ))}
            </div>
            <Btn
              size="sm"
              className="ol-vocab-select-mode"
              style={{ paddingInline: 14 }}
              onClick={() => {
                setSelecting((current) => {
                  if (current) setSelectedIds(new Set());
                  return !current;
                });
              }}
            >
              <span className="ol-vocab-reserved-label">
                <span aria-hidden="true">{t('vocab.selecting')}</span>
                <span aria-hidden="true">{t('vocab.doneSelecting')}</span>
                <span>{selecting ? t('vocab.doneSelecting') : t('vocab.selecting')}</span>
              </span>
            </Btn>
          </div>
        </div>
      </div>

      {error && (
        <div
          role="alert"
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: 10,
            padding: '8px 12px',
            marginBottom: 12,
            borderRadius: 'var(--ol-r-md)',
            border: '0.5px solid rgba(239,68,68,0.22)',
            background: 'rgba(239,68,68,0.07)',
            color: 'var(--ol-red, #ef4444)',
            fontSize: 12,
            lineHeight: 1.5,
          }}
        >
          <span>{error}</span>
        </div>
      )}

      {/* Under the auto-added filter, give "delete all" a stable outlet (the trust
          prerequisite; see the comment above). */}
      {filter === 'auto' && learnedEntries.length > 0 && (
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 10 }}>
          <span style={{ fontSize: 12, color: 'var(--ol-ink-3)' }}>
            {t('vocab.learnedSection', { count: learnedEntries.length })}
          </span>
          <Btn size="sm" disabled={batchBusy} onClick={() => void onRemoveAllLearnedEntries()}>
            {t('vocab.removeAllLearned')}
          </Btn>
        </div>
      )}

      {/* Independent scroll region: with many entries only this cell scrolls, while the
          bottom panel stays pinned to the viewport bottom edge with an opaque background
          that naturally covers content scrolling beneath; this also eliminates at the root
          the previous glitch where the grid, as a page-root flex item, got squashed and its
          content overflowed behind the section below. */}
      <div
        className="ol-thinscroll ol-vocab-scroll"
        style={{ flex: 1, minHeight: 0, overflowY: 'auto', paddingRight: 2, paddingBottom: 12 }}
      >
        <div className="ol-grid-12 ol-vocab-grid">
          {loading && (
            <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
          )}
          {!loading && !error && visibleEntries.length === 0 && (
            <div style={{ fontSize: 12, color: 'var(--ol-ink-4)', gridColumn: '1 / -1' }}>
              {needle ? t('vocab.searchEmpty') : t('vocab.empty')}
            </div>
          )}
          {visibleEntries.map((entry) => (
            <VocabularyWordCard
              key={entry.id}
              entry={entry}
              auto={sourceOf(entry) === 'auto'}
              removing={removingIds.has(entry.id) || batchBusy}
              selected={selectedIds.has(entry.id)}
              selecting={selecting}
              onSelect={() => toggleSelection(entry.id)}
              cardRef={(element) => {
                if (element) cardRefs.current.set(entry.id, element);
                else cardRefs.current.delete(entry.id);
              }}
              onToggle={() => void onToggle(entry)}
              onEdit={() => openEdit(entry)}
              onRemove={() => void removeEntries([entry.id], entry)}
            />
          ))}
        </div>
      </div>

      {/* Edit-entry modal */}
      {editMount.mounted && (
        <ModalShell
          title={t('vocab.editTitle')}
          closing={editMount.closing}
          onClose={() => setEditingEntry(null)}
        >
          <TextField
            autoFocus
            value={editDraft}
            onChange={(e) => {
              setEditDraft(e.target.value);
              setEditError(null);
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                void saveEdit();
              }
            }}
            style={{ height: 40 }}
          />
          {editError && (
            <div style={{ marginTop: 8, fontSize: 12, color: 'var(--ol-red, #ef4444)' }}>
              {editError}
            </div>
          )}
          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 18 }}>
            <Btn variant="ghost" onClick={() => setEditingEntry(null)}>
              {t('common.cancel')}
            </Btn>
            <Btn variant="primary" onClick={() => void saveEdit()}>
              {t('vocab.editSave')}
            </Btn>
          </div>
        </ModalShell>
      )}

      {/* New-word modal: direct input + preset template multi-select */}
      {newWordMount.mounted && (
        <ModalShell
          title={t('vocab.newWordTitle')}
          desc={t('vocab.newWordDesc')}
          closing={newWordMount.closing}
          onClose={() => setNewWordOpen(false)}
        >
          <div style={{ display: 'flex', gap: 8 }}>
            <TextField
              autoFocus
              value={newWordDraft}
              onChange={(e) => setNewWordDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault();
                  void addNewWord();
                }
              }}
              placeholder={t('vocab.newWordInputPlaceholder')}
              style={{ height: 38, flex: 1 }}
            />
            <Btn
              variant="primary"
              icon={wordAdded ? 'check' : 'plus'}
              onClick={() => void addNewWord()}
              style={{
                borderRadius: 8,
                height: 38,
                minWidth: 92,
                justifyContent: 'center',
              }}
            >
              {wordAdded ? t('common.saved') : t('common.add')}
            </Btn>
          </div>
          <div
            style={{
              marginTop: 16,
              fontSize: 12.5,
              fontWeight: 'var(--ol-font-weight-semibold)',
              color: 'var(--ol-ink-2)',
            }}
          >
            {t('vocab.newWordTemplates')}
          </div>
          <div style={{ marginTop: 8, display: 'grid', gap: 8 }}>
            {presets.map((p) => {
              const checked = newWordTemplateIds.includes(p.id);
              return (
                <button
                  key={p.id}
                  type="button"
                  className="ol-pressable"
                  aria-pressed={checked}
                  onClick={() =>
                    setNewWordTemplateIds((prev) =>
                      prev.includes(p.id) ? prev.filter((x) => x !== p.id) : [...prev, p.id],
                    )
                  }
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 10,
                    textAlign: 'left',
                    padding: '10px 12px',
                    borderRadius: 'var(--ol-r-md)',
                    fontFamily: 'inherit',
                    border: checked
                      ? '1px solid var(--ol-blue)'
                      : '0.5px solid var(--ol-line-strong)',
                    background: checked ? 'var(--ol-blue-soft)' : 'var(--ol-surface)',
                    cursor: 'default',
                    transition:
                      'background 0.14s var(--ol-motion-quick), border-color 0.14s var(--ol-motion-quick), transform 0.34s cubic-bezier(0.34, 1.56, 0.64, 1)',
                  }}
                >
                  <span style={{ minWidth: 0, flex: 1 }}>
                    <span
                      style={{
                        display: 'block',
                        fontSize: 13,
                        fontWeight: 'var(--ol-font-weight-semibold)',
                        color: 'var(--ol-ink)',
                      }}
                    >
                      {p.name}
                    </span>
                    <span
                      style={{
                        display: 'block',
                        marginTop: 2,
                        fontSize: 11.5,
                        color: 'var(--ol-ink-4)',
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                        whiteSpace: 'nowrap',
                      }}
                    >
                      {p.phrases.join(' · ')}
                    </span>
                  </span>
                  <span style={{ flexShrink: 0, fontSize: 11.5, color: 'var(--ol-ink-4)' }}>
                    {t('vocab.newWordTemplateCount', { count: p.phrases.length })}
                  </span>
                  {checked && <Icon name="check" size={14} />}
                </button>
              );
            })}
          </div>
          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 18 }}>
            <Btn variant="ghost" onClick={() => setNewWordOpen(false)}>
              {t('common.close')}
            </Btn>
            <Btn
              variant="primary"
              onClick={() => void applyNewWordTemplates()}
              disabled={newWordTemplateIds.length === 0}
            >
              {t('vocab.newWordAddSelected')}
            </Btn>
          </div>
        </ModalShell>
      )}
    </div>
  );
}

interface ModalShellProps {
  title: string;
  desc?: string;
  /** Keeps the closing card mounted until its exit finishes. */
  closing?: boolean;
  onClose: () => void;
  children: React.ReactNode;
}

function ModalShell({ title, desc, closing = false, onClose, children }: ModalShellProps) {
  const { t } = useTranslation();
  const titleId = useId();
  const descId = useId();
  return (
    <Modal
      onClose={onClose}
      closing={closing}
      width="min(440px, 100%)"
      style={{ padding: 20 }}
      motionVariant="pop"
      backdrop="blur-only"
      cardClassName="ol-vocab-dialog"
      labelledBy={titleId}
      describedBy={desc ? descId : undefined}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'flex-start',
          justifyContent: 'space-between',
          gap: 12,
          marginBottom: desc ? 4 : 14,
        }}
      >
        <div style={{ minWidth: 0 }}>
          <div
            id={titleId}
            style={{
              fontSize: 15,
              fontWeight: 'var(--ol-font-weight-demibold)',
              color: 'var(--ol-ink)',
            }}
          >
            {title}
          </div>
          {desc && (
            <div
              id={descId}
              style={{ marginTop: 4, fontSize: 12.5, color: 'var(--ol-ink-3)', lineHeight: 1.5 }}
            >
              {desc}
            </div>
          )}
        </div>
        <button
          type="button"
          className="ol-pressable"
          aria-label={t('common.close')}
          onClick={onClose}
          style={{
            width: 26,
            height: 26,
            flexShrink: 0,
            border: 0,
            borderRadius: 'var(--ol-control-radius)',
            background: 'transparent',
            color: 'var(--ol-ink-4)',
            cursor: 'default',
            display: 'inline-flex',
            alignItems: 'center',
            justifyContent: 'center',
            transition: 'transform 0.34s cubic-bezier(0.34, 1.56, 0.64, 1)',
          }}
        >
          <Icon name="close" size={14} />
        </button>
      </div>
      <div style={{ marginTop: 12 }}>{children}</div>
    </Modal>
  );
}
