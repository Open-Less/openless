import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { RestorePreview, SyncConflictChoice } from '../../api/cloud-sync';
import { Button as Btn } from '../../ui/controls/Button';

const categoryKeys = [
  'preferences',
  'ui_preferences',
  'channels',
  'provider_credentials',
  'dictionary',
  'vocabulary_presets',
  'corrections',
  'style_packs',
  'history',
  'activity',
  'device_profile',
] as const;
function categoryKey(kind: string): string {
  return categoryKeys.some((key) => key === kind) ? kind : 'other';
}
export function RestoreReview({
  preview,
  busy,
  stale,
  onApply,
}: {
  preview: RestorePreview;
  busy: boolean;
  stale: boolean;
  onApply: (mode: 'replace' | 'merge', choices: SyncConflictChoice[]) => void;
}) {
  const { t } = useTranslation();
  const [mode, setMode] = useState<'replace' | 'merge'>('merge');
  const [reviewed, setReviewed] = useState(false);
  const [choices, setChoices] = useState<Record<string, 'local' | 'cloud'>>({});
  const [page, setPage] = useState(0);
  const pageSize = 20;
  const selected = preview.conflicts.filter(
    (conflict) => choices[conflict.id] !== undefined,
  ).length;
  const complete = mode === 'replace' || selected === preview.conflicts.length;
  return (
    <div style={{ display: 'grid', gap: 16 }}>
      <p>{t('cloudSyncE2ee.restoreDescription')}</p>
      {preview.unconfirmedOperationId && (
        <p role="note">{t('cloudSyncE2ee.errors.outcomeUnknown')}</p>
      )}
      <ul style={{ margin: 0, paddingLeft: 20, lineHeight: 1.75 }}>
        {Object.entries(preview.counts)
          .filter(([, count]) => count > 0)
          .map(([kind, count]) => (
            <li key={kind}>
              {t(`cloudSyncE2ee.categories.${categoryKey(kind)}`)}: {count}
            </li>
          ))}
      </ul>
      {preview.deviceSettingsToReview.length > 0 && (
        <p>{t('cloudSyncE2ee.deviceReview', { count: preview.deviceSettingsToReview.length })}</p>
      )}
      <fieldset
        style={{ border: 0, padding: 0, margin: 0, display: 'grid', gap: 9 }}
        disabled={busy}
      >
        <legend>{t('cloudSyncE2ee.restoreMode')}</legend>
        <label>
          <input
            type="radio"
            name="e2ee-restore-mode"
            checked={mode === 'merge'}
            onChange={() => {
              setMode('merge');
              setReviewed(false);
            }}
          />{' '}
          {t('cloudSyncE2ee.merge')}
        </label>
        <label>
          <input
            type="radio"
            name="e2ee-restore-mode"
            checked={mode === 'replace'}
            onChange={() => {
              setMode('replace');
              setReviewed(false);
            }}
          />{' '}
          {t('cloudSyncE2ee.replace')}
        </label>
      </fieldset>
      {mode === 'replace' ? (
        <p style={{ color: 'var(--ol-err)' }}>{t('cloudSyncE2ee.replaceWarning')}</p>
      ) : (
        preview.conflicts.length > 0 && (
          <>
            <p>
              {t('cloudSyncE2ee.conflictProgress', { selected, total: preview.conflicts.length })}
            </p>
            {preview.conflicts
              .slice(page * pageSize, (page + 1) * pageSize)
              .map((conflict, index) => (
                <fieldset
                  key={conflict.id}
                  disabled={busy}
                  style={{
                    border: '1px solid var(--ol-line)',
                    borderRadius: 'var(--ol-r-md)',
                    padding: 12,
                  }}
                >
                  <legend>
                    {t('cloudSyncE2ee.conflictItem', {
                      index: page * pageSize + index + 1,
                      category: t(`cloudSyncE2ee.categories.${categoryKey(conflict.kind)}`),
                    })}
                  </legend>
                  <p>
                    {t(
                      `cloudSyncE2ee.conflictReasons.${['both_modified', 'delete_modify', 'no_common_baseline'].includes(conflict.reason) ? conflict.reason : 'other'}`,
                    )}
                  </p>
                  <div style={{ display: 'flex', flexWrap: 'wrap', gap: 16, marginTop: 8 }}>
                    {(['local', 'cloud'] as const).map((side) => (
                      <label key={side}>
                        <input
                          type="radio"
                          name={`e2ee-conflict-${page * pageSize + index}`}
                          checked={choices[conflict.id] === side}
                          onChange={() => {
                            setChoices((current) => ({ ...current, [conflict.id]: side }));
                            setReviewed(false);
                          }}
                        />{' '}
                        {t(`cloudSyncE2ee.conflict${side === 'local' ? 'Local' : 'Cloud'}`)}
                      </label>
                    ))}
                  </div>
                </fieldset>
              ))}
            {preview.conflicts.length > pageSize && (
              <div className="ol-cloud-sync-actions">
                <Btn disabled={page === 0 || busy} onClick={() => setPage((value) => value - 1)}>
                  {t('cloudSyncE2ee.previous')}
                </Btn>
                <span>
                  {page + 1} / {Math.ceil(preview.conflicts.length / pageSize)}
                </span>
                <Btn
                  disabled={(page + 1) * pageSize >= preview.conflicts.length || busy}
                  onClick={() => setPage((value) => value + 1)}
                >
                  {t('cloudSyncE2ee.next')}
                </Btn>
              </div>
            )}
          </>
        )
      )}
      <label style={{ display: 'flex', alignItems: 'flex-start', gap: 8 }}>
        <input
          type="checkbox"
          checked={reviewed}
          disabled={busy || stale}
          onChange={(event) => setReviewed(event.currentTarget.checked)}
        />
        {t('cloudSyncE2ee.restoreCheck')}
      </label>
      {stale && (
        <p role="alert" style={{ color: 'var(--ol-err)' }}>
          {t('cloudSyncE2ee.errors.changed')}
        </p>
      )}
      <Btn
        variant="primary"
        disabled={busy || stale || !reviewed || !complete}
        onClick={() =>
          onApply(
            mode,
            mode === 'replace'
              ? []
              : preview.conflicts.map((conflict) => ({
                  id: conflict.id,
                  side: choices[conflict.id],
                })),
          )
        }
      >
        {t('cloudSyncE2ee.applyRestore')}
      </Btn>
    </div>
  );
}
