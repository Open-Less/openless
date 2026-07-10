// 服务 → 扩展市场：通过 GitHub 登录获取上传 / 点赞身份。
// 浏览与安装风格在「风格」页内完成，设置页只管登录身份。
//
// 登录走共用的 <GithubLoginModal />（GitHub OAuth Device Flow），与风格市场
// 完全一致 —— 点登录弹出统一登录窗口，授权成功写回 prefs.marketplaceDevLogin。

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Icon } from '../../components/Icon';
import { GithubLoginModal } from '../../components/GithubLoginModal';
import { Btn, Card } from '../_atoms';
import { SectionTitle } from './shared';

export function MarketplaceSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs: savePrefs } = useHotkeySettings();
  const [showLogin, setShowLogin] = useState(false);

  if (!prefs) {
    return (
      <Card>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
      </Card>
    );
  }

  const login = prefs.marketplaceDevLogin.trim();
  const signedIn = login.length > 0;

  const signOut = () => {
    void savePrefs(current => ({ ...current, marketplaceDevLogin: '' }));
  };

  return (
    <Card>
      <SectionTitle>{t('settings.marketplace.title')}</SectionTitle>

      {signedIn ? (
        /* ── 已登录 ──────────────────────────────────────────────── */
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 12,
            padding: '10px 12px',
            borderRadius: 10,
            background: 'var(--ol-surface-2)',
            border: '0.5px solid var(--ol-line)',
          }}
        >
          <div
            style={{
              width: 34,
              height: 34,
              borderRadius: 999,
              background: 'var(--ol-primary-solid-bg)',
              color: 'var(--ol-primary-solid-ink)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              flexShrink: 0,
            }}
          >
            <Icon name="user" size={17} />
          </div>
          <div style={{ minWidth: 0, flex: 1 }}>
            <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)' }}>
              {t('settings.marketplace.github.signedIn')}
            </div>
            <div
              style={{
                fontSize: 13.5,
                fontWeight: 600,
                color: 'var(--ol-ink)',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
              }}
            >
              @{login}
            </div>
          </div>
          <Btn variant="ghost" size="sm" onClick={signOut}>
            {t('settings.marketplace.github.signOut')}
          </Btn>
        </div>
      ) : (
        /* ── 未登录 ──────────────────────────────────────────────── */
        <div>
          <Btn variant="primary" size="sm" icon="user" onClick={() => setShowLogin(true)}>
            {t('settings.marketplace.github.signIn')}
          </Btn>
        </div>
      )}

      {showLogin && (
        <GithubLoginModal
          onClose={() => setShowLogin(false)}
          onSuccess={nextLogin => {
            void savePrefs(current => ({ ...current, marketplaceDevLogin: nextLogin }))
              .catch(e => console.warn('[marketplace] save login to prefs failed', e));
          }}
        />
      )}
    </Card>
  );
}
