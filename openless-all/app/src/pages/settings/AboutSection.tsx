// 关于 → 版本信息 / 检查更新 / 字体大小 / 文档链接。
// 「个性化」原本是独立 tab，但只剩字体大小一项、整页太空，遂并入「关于」。
// 「加入 Beta 渠道」已挪到「高级」页底部（见 BetaChannelSection），这里图标旁
// 只保留查正式版的「检查更新」按钮。

import { useEffect, useRef, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { Row } from '../../components/ui/Row';
import { getPlatformCapabilities, openExternal } from '../../lib/ipc';
import type { PlatformCapabilities } from '../../lib/types';
import { APP_VERSION_LABEL } from '../../lib/appVersion';
import { readAppTheme, setAppTheme, type AppThemeId } from '../../lib/appTheme';
import { readFontScale, setFontScale, type FontScaleId } from '../../lib/fontScale';
import { Card } from '../_atoms';
import { SectionTitle } from './shared';
import { CheckUpdateButton } from './CheckUpdateButton';

const HELP_URL = 'https://github.com/appergb/openless#readme';
const RELEASE_NOTES_URL = 'https://github.com/appergb/openless/releases';

export function AboutSection() {
  const { t } = useTranslation();
  const [qqCopied, setQqCopied] = useState(false);
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);
  const qqCopiedRef = useRef<number | null>(null);

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  useEffect(() => () => {
    if (qqCopiedRef.current) clearTimeout(qqCopiedRef.current);
  }, []);

  const copyQq = () => {
    navigator.clipboard?.writeText('1078960553');
    setQqCopied(true);
    if (qqCopiedRef.current) clearTimeout(qqCopiedRef.current);
    qqCopiedRef.current = window.setTimeout(() => setQqCopied(false), 1500);
  };

  return (
    <>
      {/* ─── 版本信息 + 检查更新（正式版）─────────────────────────────── */}
      <Card>
        <div style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
          <img
            src="AppIcon.png"
            alt=""
            style={{ width: 56, height: 56, borderRadius: 13, boxShadow: '0 4px 10px rgba(0,0,0,.10), 0 0 0 0.5px rgba(0,0,0,.06)' }}
          />
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ fontSize: 17, fontWeight: 600 }}>OpenLess</div>
            <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', marginTop: 2 }}>
              {t('modal.about.tagline')} · {APP_VERSION_LABEL}
            </div>
          </div>
          {/* 图标右上方：查正式版的检查更新按钮。Beta 渠道在「高级」页。 */}
          {platformCaps?.supportsAutoUpdate === true && (
            <CheckUpdateButton channel="stable" />
          )}
        </div>
      </Card>

      {/* ─── 个性化（字体大小）—— 原 personalize tab 并入此处 ──────────── */}
      <Card>
        <SectionTitle>{t('modal.sections.personalize')}</SectionTitle>
        <ThemeRow />
        <FontSizeRow />
      </Card>

      {/* ─── 文档链接 ─────────────────────────────────────────────── */}
      <Card>
        <SectionTitle>{t('settings.about.linksTitle')}</SectionTitle>
        <Row label={t('modal.about.source')}>
          <button style={btnGhost} onClick={() => openExternal('https://github.com/appergb/openless')}>
            GitHub
          </button>
        </Row>
        <Row label={t('modal.about.docs')}>
          <button style={btnGhost} onClick={() => openExternal(HELP_URL)}>
            {t('modal.about.docsBtn')}
          </button>
        </Row>
        <Row label={t('modal.sections.helpCenter')}>
          <button style={btnGhost} onClick={() => openExternal(HELP_URL)}>
            {t('modal.sections.helpCenter')}
          </button>
        </Row>
        <Row label={t('modal.sections.releaseNotes')}>
          <button style={btnGhost} onClick={() => openExternal(RELEASE_NOTES_URL)}>
            {t('modal.sections.releaseNotes')}
          </button>
        </Row>
        <Row label={t('modal.about.feedback')}>
          <button style={btnGhost} onClick={() => openExternal('https://github.com/appergb/openless/issues')}>
            {t('modal.about.feedbackBtn')}
          </button>
        </Row>
        <Row label={t('modal.about.qq')}>
          <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
            <kbd style={{
              padding: '4px 10px', fontSize: 12, fontFamily: 'var(--ol-font-mono)',
              borderRadius: 6, background: 'var(--ol-surface-2)',
              border: '0.5px solid var(--ol-line-strong)',
              boxShadow: '0 1px 0 rgba(0,0,0,0.04)',
              color: 'var(--ol-ink-2)',
            }}>1078960553</kbd>
            <button onClick={copyQq} title={t('modal.about.copyQq')} style={btnGhost}>
              <Icon name="copy" size={14} />
            </button>
            {qqCopied && <span style={{ fontSize: 11, color: 'var(--ol-ok)', whiteSpace: 'nowrap' }}>{t('common.copied')}</span>}
          </div>
        </Row>
      </Card>
    </>
  );
}

function ThemeRow() {
  const { t } = useTranslation();
  const [theme, setThemeState] = useState<AppThemeId>(() => readAppTheme());
  const themeOptions: Array<[AppThemeId, string]> = [
    ['light', t('modal.personalize.themeLight')],
    ['dark', t('modal.personalize.themeDark')],
  ];

  return (
    <Row label={t('modal.personalize.theme')} desc={t('modal.personalize.themeDesc')}>
      <div style={{ display: 'flex', gap: 4, padding: 2, background: 'var(--ol-segmented-bg)', borderRadius: 10 }}>
        {themeOptions.map(([id, label]) => {
          const selected = theme === id;
          return (
            <button
              key={id}
              onClick={() => {
                setThemeState(id);
                setAppTheme(id);
              }}
              style={{
                minWidth: 74,
                height: 30,
                border: 0,
                borderRadius: 8,
                background: selected ? 'var(--ol-segmented-active-bg)' : 'transparent',
                color: selected ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
                fontFamily: 'inherit',
                fontSize: 12,
                fontWeight: selected ? 600 : 500,
                cursor: 'default',
                boxShadow: selected ? 'var(--ol-segmented-active-shadow)' : 'none',
                transition: 'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick), box-shadow 0.18s var(--ol-motion-soft)',
                padding: '0 12px',
              }}
            >
              {label}
            </button>
          );
        })}
      </div>
    </Row>
  );
}

// 字体大小 —— 整体缩放界面字号，立即生效（fontScale.ts 走 html.style.zoom）。
function FontSizeRow() {
  const { t } = useTranslation();
  const [fontScale, setFontScaleState] = useState<FontScaleId>(() => readFontScale());
  const applyFontScaleChoice = (next: FontScaleId) => {
    setFontScaleState(next);
    setFontScale(next);
  };
  const fontOptions: Array<[FontScaleId, string]> = [
    ['small', t('modal.personalize.fontSmall')],
    ['medium', t('modal.personalize.fontMedium')],
    ['large', t('modal.personalize.fontLarge')],
  ];
  return (
    <Row label={t('modal.personalize.font')} desc={t('modal.personalize.fontDesc')}>
      <div style={{ display: 'flex', gap: 4, padding: 2, background: 'var(--ol-segmented-bg)', borderRadius: 10 }}>
        {fontOptions.map(([id, label]) => {
          const selected = fontScale === id;
          return (
            <button
              key={id}
              onClick={() => applyFontScaleChoice(id)}
              style={{
                minWidth: 64,
                height: 28,
                border: 0,
                borderRadius: 8,
                background: selected ? 'var(--ol-segmented-active-bg)' : 'transparent',
                color: selected ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
                fontFamily: 'inherit',
                fontSize: 12,
                fontWeight: selected ? 600 : 500,
                cursor: 'default',
                boxShadow: selected ? 'var(--ol-segmented-active-shadow)' : 'none',
                transition: 'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick), box-shadow 0.18s var(--ol-motion-soft)',
                padding: '0 12px',
              }}
            >
              {label}
            </button>
          );
        })}
      </div>
    </Row>
  );
}

const btnGhost: CSSProperties = {
  padding: '5px 10px', fontSize: 12, borderRadius: 6,
  border: '0.5px solid var(--ol-line-strong)',
  background: 'var(--ol-control-solid)', color: 'var(--ol-ink-2)',
  cursor: 'default', fontFamily: 'inherit',
  maxWidth: '100%',
  transition: 'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)',
};
