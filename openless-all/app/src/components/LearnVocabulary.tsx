import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { addLearnedVocab } from '../lib/ipc/vocab';
import { Btn } from '../pages/_atoms';
import { inputStyle } from '../pages/settings/shared';

/** An explicit, cross-platform alternative when a host editor cannot be read. */
export function LearnVocabulary() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [phrase, setPhrase] = useState('');
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');
  const save = async () => {
    if (busy || !phrase.trim()) return;
    setBusy(true);
    setMessage('');
    try {
      await addLearnedVocab(phrase.trim());
      setPhrase('');
      setOpen(false);
      setMessage(t('vocabLearning.saved', '已记住，可在词典中管理'));
    } catch (error) {
      setMessage(t('vocabLearning.failed', '保存失败：{{error}}', { error: String(error) }));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div style={{ marginTop: 12, minWidth: 0 }}>
      {!open ? (
        <Btn
          variant="ghost"
          size="sm"
          onClick={() => {
            setOpen(true);
            setMessage('');
          }}
        >
          {t('vocabLearning.open', '记住词汇')}
        </Btn>
      ) : (
        <div
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.nativeEvent.isComposing) {
              e.preventDefault();
              void save();
            }
          }}
        >
          <label style={{ display: 'grid', gap: 8 }}>
            {t('vocabLearning.label', '输入要记住的正确词汇')}
            <input
              autoFocus
              value={phrase}
              maxLength={64}
              disabled={busy}
              onChange={(e) => setPhrase(e.target.value)}
              style={{ ...inputStyle, width: '100%', minWidth: 0 }}
            />
          </label>
          <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', marginTop: 8 }}>
            <Btn disabled={busy || !phrase.trim()} onClick={() => void save()}>
              {t('vocabLearning.confirm', '确认加入词典')}
            </Btn>
            <Btn
              variant="ghost"
              disabled={busy}
              onClick={() => {
                setOpen(false);
                setPhrase('');
                setMessage('');
              }}
            >
              {t('common.cancel')}
            </Btn>
          </div>
        </div>
      )}
      {message && (
        <div role="status" style={{ marginTop: 8, overflowWrap: 'anywhere' }}>
          {message}
        </div>
      )}
    </div>
  );
}
