// About → version info / check updates / font size / doc links.
// "Personalization" was once its own tab but was left with only font size, so it merged into "About".
// "Join the Beta channel" moved to the bottom of the Advanced page (see BetaChannelSection); here the
// icon area keeps only the stable-channel "check updates" button.

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Row } from '../../../components/ui/Row';
import { getPlatformCapabilities } from '../../../runtime/platform';
import { openExternal } from '../../../api/files';
import type { PlatformCapabilities } from '../../../api/contracts';
import { APP_VERSION_LABEL } from '../../../lib/appVersion';
import { Card } from '../../../ui/layout/Card';
import { useConservativeLayout, useLayoutStack } from '../../../lib/useMobileLayout';
import { btnGhostStyle } from '../../../ui/controls/settingsFieldStyles';
import { SectionTitle } from '../../../ui/layout/SectionTitle';
import { officialLinks } from '../../../lib/officialLinks';
import { CheckUpdateButton } from './CheckUpdateButton';

const RELEASE_NOTES_URL = 'https://github.com/Open-Less/openless/releases';

export function AboutSection() {
  const { t, i18n } = useTranslation();
  const links = officialLinks(i18n.language);
  const baseLayoutStack = useLayoutStack();
  const conservativeLayout = useConservativeLayout();
  const compactLayout = baseLayoutStack || conservativeLayout;
  const appIconSize = compactLayout ? 36 : 56;
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);

  useEffect(() => {
    let active = true;
    void getPlatformCapabilities()
      .then((capabilities) => {
        if (active) setPlatformCaps(capabilities);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, []);

  return (
    <>
      {/* ─── Version info + check updates (stable) ─────────────────────── */}
      <Card>
        <div
          className="ol-inline-composite"
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: compactLayout ? 4 : 14,
            minWidth: 0,
            flexWrap: 'nowrap',
          }}
        >
          <img
            src="AppIcon.png"
            alt=""
            style={{
              width: appIconSize,
              height: appIconSize,
              flexShrink: 0,
              borderRadius: 'var(--ol-radius)',
              boxShadow: '0 4px 10px rgba(0,0,0,.10), 0 0 0 0.5px rgba(0,0,0,.06)',
            }}
          />
          <div style={{ flex: 1, minWidth: 0 }}>
            <div
              style={{
                fontSize: 17,
                fontWeight: 'var(--ol-font-weight-semibold)',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
              }}
            >
              OpenLess
            </div>
            <div
              style={{
                fontSize: 12,
                color: 'var(--ol-ink-3)',
                marginTop: 2,
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
              }}
            >
              {t('modal.about.tagline')} · {APP_VERSION_LABEL}
            </div>
          </div>
          {/* Top right of the icon: stable-channel check-updates button. The Beta section has its own channel check. */}
          {platformCaps?.supportsAutoUpdate === true && (
            <CheckUpdateButton channel="stable" compact={compactLayout} />
          )}
        </div>
      </Card>

      {/* Personalization (font size) removed as requested (page slimming). */}

      {/* ─── Documentation links ─────────────────────────────────────── */}
      <Card>
        <SectionTitle>{t('settings.about.linksTitle')}</SectionTitle>
        <Row label={t('modal.about.source')}>
          <button
            style={btnGhostStyle}
            onClick={() => openExternal('https://github.com/Open-Less/openless')}
          >
            GitHub
          </button>
        </Row>
        <Row label={t('modal.about.docs')}>
          <button style={btnGhostStyle} onClick={() => openExternal(links.docs)}>
            {t('modal.about.docs')}
          </button>
        </Row>
        <Row label={t('modal.sections.helpCenter')}>
          <button style={btnGhostStyle} onClick={() => openExternal(links.docs)}>
            {t('modal.sections.helpCenter')}
          </button>
        </Row>
        <Row label={t('modal.sections.releaseNotes')}>
          <button style={btnGhostStyle} onClick={() => openExternal(RELEASE_NOTES_URL)}>
            {t('modal.sections.releaseNotes')}
          </button>
        </Row>
        <Row label={t('shell.footer.helpPopover.credits')}>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
            <button
              style={btnGhostStyle}
              title="Rare UI GitHub Activity — MIT License + Commons Clause"
              onClick={() => void openExternal('https://rareui.com/components/githubactivity')}
            >
              Rare UI · GitHub Activity
            </button>
            <button
              style={btnGhostStyle}
              title="Rare UI Password Input — MIT License + Commons Clause"
              onClick={() => void openExternal('https://www.rareui.com/components/passwordinput')}
            >
              Rare UI · Password Input
            </button>
            <button
              style={btnGhostStyle}
              title="Rare UI Toast — MIT License + Commons Clause"
              onClick={() => void openExternal('https://www.rareui.com/components/toast')}
            >
              Rare UI · Toast
            </button>
          </div>
        </Row>
        <Row label={t('modal.about.feedback')}>
          <button
            style={btnGhostStyle}
            onClick={() => openExternal('https://github.com/Open-Less/openless/issues')}
          >
            {t('modal.about.feedbackBtn')}
          </button>
        </Row>
        <Row label={t('modal.about.qq')}>
          <button style={btnGhostStyle} onClick={() => void openExternal(links.community)}>
            {t('modal.about.qq')} ↗
          </button>
        </Row>
      </Card>
    </>
  );
}
