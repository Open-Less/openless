// PrivacyTab — 设置弹窗「隐私」：本地优先说明 + 权限管理 · 数据存储。

import { useTranslation } from 'react-i18next';
import { DataStorageSection } from './DataStorageSection';
import { PermissionsSection } from './PermissionsSection';

export function PrivacyTab() {
  const { t } = useTranslation();
  return (
    <>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 10,
          padding: '10px 12px',
          borderRadius: 10,
          background: 'var(--ol-blue-soft)',
          marginBottom: 2,
        }}
      >
        <span style={{
          fontSize: 11, padding: '3px 8px', borderRadius: 999,
          background: 'var(--ol-surface)',
          color: 'var(--ol-blue)', fontWeight: 600, flexShrink: 0,
        }}>
          {t('modal.about.localFirst')}
        </span>
        <span style={{ fontSize: 11.5, color: 'var(--ol-ink-3)', lineHeight: 1.55 }}>
          {t('modal.about.privacyDesc')}
        </span>
      </div>
      <PermissionsSection />
      <DataStorageSection />
    </>
  );
}
