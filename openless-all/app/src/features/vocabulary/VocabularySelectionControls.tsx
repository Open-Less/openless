import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { Button } from '../../ui/controls/Button';
import { useVocabularyLongPress } from './useVocabularyLongPress';

export function VocabularySelectionToggle({
  selecting,
  disabled,
  onToggle,
  onSelectAll,
}: {
  selecting: boolean;
  disabled: boolean;
  onToggle: () => void;
  onSelectAll: () => void;
}) {
  const { t } = useTranslation();
  const hold = useVocabularyLongPress(!selecting || disabled, onSelectAll);
  return (
    <div
      {...hold}
      className="ol-vocab-select-slot"
      data-selecting={selecting ? '' : undefined}
      onContextMenu={(event) => {
        if (selecting) event.preventDefault();
      }}
    >
      <span className="ol-vocab-select-width" aria-hidden="true">
        {t('vocab.selecting')}
      </span>
      <Button
        size="sm"
        className="ol-vocab-select-mode"
        ariaLabel={selecting ? t('vocab.doneSelecting') : t('vocab.selecting')}
        style={selecting ? { padding: 0 } : undefined}
        disabled={disabled}
        onClick={onToggle}
      >
        {selecting ? <Icon name="x" size={16} /> : t('vocab.selecting')}
      </Button>
    </div>
  );
}

export function VocabularySelectionFooter({
  selecting,
  batchBusy,
  canSelectMore,
  selectedCount,
  onSelectAll,
  onRemove,
}: {
  selecting: boolean;
  batchBusy: boolean;
  canSelectMore: boolean;
  selectedCount: number;
  onSelectAll: () => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation();
  const removeLabel = t('vocab.deleteSelected', { count: selectedCount });
  return (
    <div
      className="ol-vocab-selection-footer"
      data-selecting={selecting ? '' : undefined}
      aria-hidden={!selecting}
    >
      <Button
        size="sm"
        className="ol-vocab-batch-all"
        icon="check"
        disabled={!selecting || batchBusy || !canSelectMore}
        onClick={onSelectAll}
      >
        {t('vocab.selectAllVisible')}
      </Button>
      <span className="ol-vocab-batch-remove" data-empty={selectedCount === 0 ? '' : undefined}>
        <Button
          size="sm"
          icon="trash"
          ariaLabel={removeLabel}
          title={removeLabel}
          disabled={!selecting || batchBusy || selectedCount === 0}
          onClick={onRemove}
        />
      </span>
    </div>
  );
}
