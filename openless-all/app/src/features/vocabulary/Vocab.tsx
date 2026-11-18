import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import type { DictionaryEntry } from '../../api/contracts';
import { ExpandingSearch } from '../../components/ExpandingSearch';
import { Icon } from '../../components/Icon';
import { Tooltip } from '../../components/Tooltip';
import { Button as Btn } from '../../ui/controls/Button';
import { PageHeader } from '../../ui/layout/PageHeader';
import { Modal } from '../../ui/overlays/Modal';
import { useVocabularyWorkspace } from './useVocabularyWorkspace';

export function Vocab() {
  const {
    t,
    mobile,
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
  return (
    <div style={{ display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0 }}>
      <PageHeader
        kicker={t('vocab.kicker')}
        title={t('vocab.title')}
        desc={t('vocab.desc')}
        right={
          <div style={{ display: 'flex', gap: 8, alignItems: 'flex-start' }}>
            {selectedIds.size > 0 && (
              <button
                type="button"
                className="ol-vocab-delete-selected"
                disabled={batchBusy}
                onClick={() => void removeEntries([...selectedIds])}
              >
                <Icon name="trash" size={15} />
                {t('vocab.deleteSelected', { count: selectedIds.size })}
              </button>
            )}
            <Btn
              variant="primary"
              icon="plus"
              onClick={() => {
                setNewWordOpen(true);
                setNewWordDraft('');
                setNewWordTemplateIds([]);
              }}
            >
              {t('vocab.newWord')}
            </Btn>
          </div>
        }
      />

      {/* Toolbar: source segment filter + circular search (expands leftward on click). */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 12,
          marginBottom: 14,
          flexWrap: 'wrap',
          flexShrink: 0,
        }}
      >
        <div className="ol-seg" role="tablist" aria-label={t('vocab.title')}>
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
          onClick={() => {
            setSelecting((current) => {
              if (current) setSelectedIds(new Set());
              return !current;
            });
          }}
        >
          {selecting ? t('vocab.doneSelecting') : t('vocab.selecting')}
        </Btn>
        <label className="ol-vocab-select-all" hidden={!selecting}>
          <input
            type="checkbox"
            disabled={batchBusy || visibleEntries.length === 0}
            checked={
              visibleEntries.length > 0 &&
              visibleEntries.every((entry) => selectedIds.has(entry.id))
            }
            ref={(element) => {
              if (element)
                element.indeterminate =
                  visibleEntries.some((entry) => selectedIds.has(entry.id)) &&
                  !visibleEntries.every((entry) => selectedIds.has(entry.id));
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
          {selectedIds.size
            ? t('vocab.selectedCount', { count: selectedIds.size })
            : t('vocab.selectAllVisible')}
        </label>
        <div style={{ flex: 1 }} />
        <ExpandingSearch
          value={query}
          onChange={setQuery}
          placeholder={t('vocab.searchPlaceholder')}
          label={t('vocab.searchPlaceholder')}
        />
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
        className="ol-thinscroll"
        style={{ flex: 1, minHeight: 0, overflowY: 'auto', paddingRight: 2, paddingBottom: 12 }}
      >
        {/* Word grid: hover dims cards and reveals edit/delete on the right. */}
        <div
          className="ol-grid-12 ol-grid-cards"
          data-columns={mobile ? '1' : undefined}
          style={{ minHeight: 80, alignContent: 'start' }}
        >
          {loading && (
            <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
          )}
          {!loading && !error && visibleEntries.length === 0 && (
            <div style={{ fontSize: 12, color: 'var(--ol-ink-4)', gridColumn: '1 / -1' }}>
              {needle ? t('vocab.searchEmpty') : t('vocab.empty')}
            </div>
          )}
          {visibleEntries.map((entry) => (
            <WordCard
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
          <input
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
            style={{
              width: '100%',
              boxSizing: 'border-box',
              height: 40,
              padding: '0 12px',
              border: '1.5px solid var(--ol-ink)',
              borderRadius: 'var(--ol-r-md)',
              fontSize: 14,
              fontFamily: 'inherit',
              outline: 'none',
              background: 'var(--ol-surface)',
              color: 'var(--ol-ink)',
            }}
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
            <input
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
              style={{
                flex: 1,
                minWidth: 0,
                height: 38,
                padding: '0 12px',
                border: '0.5px solid var(--ol-line-strong)',
                borderRadius: 'var(--ol-radius)',
                fontSize: 13.5,
                fontFamily: 'inherit',
                outline: 'none',
                background: 'var(--ol-surface-2)',
                color: 'var(--ol-ink)',
              }}
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

      <style>{`
        @keyframes ol-chip-in {
          from { opacity: 0; transform: scale(.97); }
          to   { opacity: 1; transform: scale(1); }
        }
      `}</style>
    </div>
  );
}

interface WordCardProps {
  entry: DictionaryEntry;
  auto: boolean;
  /** Delete exit animation in progress: block interaction to prevent repeated clicks. */
  removing: boolean;
  cardRef: (element: HTMLDivElement | null) => void;
  onToggle: () => void;
  onEdit: () => void;
  onRemove: () => void;
  selected: boolean;
  selecting: boolean;
  onSelect: () => void;
}

/** Word card: shows icon + text + hit count by default; hover/focus-within dims it and
    reveals edit/delete. */
function WordCard({
  entry,
  auto,
  removing,
  selected,
  selecting,
  cardRef,
  onToggle,
  onEdit,
  onRemove,
  onSelect,
}: WordCardProps) {
  const { t } = useTranslation();
  const enabled = entry.enabled;
  return (
    <div
      ref={cardRef}
      className="ol-word-card"
      data-disabled={enabled ? undefined : 'true'}
      data-selected={selected ? 'true' : undefined}
      data-selecting={selecting ? 'true' : undefined}
      style={removing ? { pointerEvents: 'none' } : undefined}
    >
      <span className="ol-word-card-icon" aria-hidden>
        <Icon name={auto ? 'sparkle' : 'feather'} size={14} />
      </span>
      <button type="button" className="ol-word-card-text" onClick={onToggle} title={entry.phrase}>
        {entry.phrase}
      </button>
      {!enabled && <span className="ol-word-card-state">{t('vocab.disabledWord')}</span>}
      <span className="ol-word-card-hits">{entry.hits}</span>
      <span className="ol-word-card-actions">
        <Tooltip content={t('vocab.edit')} placement="top">
          <button
            type="button"
            className="ol-word-card-action"
            aria-label={t('vocab.edit')}
            disabled={removing}
            onClick={onEdit}
          >
            <Icon name="pencil" size={14} />
          </button>
        </Tooltip>
        <Tooltip content={t('common.delete')} placement="top">
          <button
            type="button"
            className="ol-word-card-action"
            aria-label={t('vocab.deleteWord', { phrase: entry.phrase })}
            disabled={removing}
            onClick={onRemove}
          >
            <Icon name="trash" size={14} />
          </button>
        </Tooltip>
      </span>
      <input
        type="checkbox"
        className="ol-word-card-select"
        checked={selected}
        disabled={removing}
        aria-label={t('vocab.selectWord', { phrase: entry.phrase })}
        onChange={onSelect}
      />
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

/** Page-level mini modal: backdrop fade-in + card spring pop-out; closes on Esc or backdrop click. */
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
