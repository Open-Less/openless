import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { openAskWindow } from '../../api/ask-window';
import { Button } from '../../ui/controls/Button';
import { Card } from '../../ui/layout/Card';
import { PageHeader } from '../../ui/layout/PageHeader';

export function AskTool() {
  const { t } = useTranslation();
  const [error, setError] = useState('');
  const open = () => { setError(''); void openAskWindow().catch((reason) => setError(String(reason))); };
  useEffect(() => { open(); }, []);
  return <><PageHeader title={t('selectionAsk.title', 'Ask')} /><Card>
    <Button onClick={open}>{t('selectionAsk.title', 'Ask')}</Button>
    {error && <p role="alert">{error}</p>}
  </Card></>;
}
