// About → version info / check updates / font size / doc links.
// "Personalization" was once its own tab but was left with only font size, so it merged into "About".
// "Join the Beta channel" moved to the bottom of the Advanced page (see BetaChannelSection); here the
// icon area keeps only the stable-channel "check updates" button.

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../../components/Icon';
import { Row } from '../../../components/ui/Row';
import { getPlatformCapabilities } from '../../../runtime/platform';
import { copyTextToClipboard, openExternal } from '../../../api/files';
import type { PlatformCapabilities } from '../../../api/contracts';
import { APP_VERSION_LABEL } from '../../../lib/appVersion';
import { Card } from '../../../ui/layout/Card';
import { useConservativeLayout, useLayoutStack } from '../../../lib/useMobileLayout';
import { btnGhostStyle } from '../../../ui/controls/settingsFieldStyles';
import { SectionTitle } from '../../../ui/layout/SectionTitle';
import { useCopyFeedback } from '../../../ui/feedback/useCopyFeedback';
import { useInteraction } from '../../../ui/overlays/InteractionProvider';
import { CheckUpdateButton } from './CheckUpdateButton';

const HELP_URL = 'https://github.com/Open-Less/openless#readme';
const RELEASE_NOTES_URL = 'https://github.com/Open-Less/openless/releases';

export function AboutSection() {
  const { t } = useTranslation();
  const baseLayoutStack = useLayoutStack();
  const conservativeLayout = useConservativeLayout();
  const compactLayout = baseLayoutStack || conservativeLayout;
  const appIconSize = compactLayout ? 36 : 56;
  const { notify } = useInteraction();
  const { copied, copy } = useCopyFeedback(copyTextToClipboard, () =>
    notify({ message: t('common.operationFailed'), state: 'failed' }),
  );
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
          <button style={btnGhostStyle} onClick={() => openExternal(HELP_URL)}>
            {t('modal.about.docsBtn')}
          </button>
        </Row>
        <Row label={t('modal.sections.helpCenter')}>
          <button style={btnGhostStyle} onClick={() => openExternal(HELP_URL)}>
            {t('modal.sections.helpCenter')}
          </button>
        </Row>
        <Row label={t('modal.sections.releaseNotes')}>
          <button style={btnGhostStyle} onClick={() => openExternal(RELEASE_NOTES_URL)}>
            {t('modal.sections.releaseNotes')}
          </button>
        </Row>
        {/* Required credit for the ported yearly activity component. */}
        <Row label={t('modal.about.credits')}>
          <button
            style={btnGhostStyle}
            title="Rare UI GitHub Activity — MIT License + Commons Clause"
            onClick={() => void openExternal('https://rareui.com/components/githubactivity')}
          >
            Rare UI · GitHub Activity
          </button>
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
          <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
            <kbd
              style={{
                padding: '4px 10px',
                fontSize: 12,
                fontFamily: 'var(--ol-font-mono)',
                borderRadius: 'var(--ol-r-sm)',
                background: 'var(--ol-surface-2)',
                border: '0.5px solid var(--ol-line-strong)',
                boxShadow: '0 1px 0 rgba(0,0,0,0.04)',
                color: 'var(--ol-ink-2)',
              }}
            >
              1078960553
            </kbd>
            <button
              onClick={() => void copy('qq', '1078960553')}
              title={t('modal.about.copyQq')}
              style={btnGhostStyle}
            >
              <Icon name="copy" size={14} />
            </button>
            {copied === 'qq' && (
              <span style={{ fontSize: 11, color: 'var(--ol-ok)', whiteSpace: 'nowrap' }}>
                {t('common.copied')}
              </span>
            )}
          </div>
        </Row>
      </Card>
    </>
  );
}
