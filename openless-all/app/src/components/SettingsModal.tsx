// Settings categories reuse the existing consumers; complex experiments open in the right pane.
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
} from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from './Icon';
import { SavedToast } from './SavedToast';
import { useSavedToastListener } from '../lib/savedEvent';
import { openExternal } from '../lib/ipc';
import { getPlatformCapabilities, getCachedPlatformCapabilities } from '../lib/platform';
import { useMobileLayout, useConservativeLayout } from '../lib/useMobileLayout';
import type { OS } from './WindowChrome';
import {
  AboutTab,
  GeneralTab,
  InputMethodTab,
  ServicesTab,
  PrivacyTab,
  AdvancedTab,
  ShortcutsTab,
  AppearanceTab,
} from '../pages/settings/tabs';
import {
  searchSettingsSections,
  visibleSettingsSections,
  visibleAdvancedPages,
  type AdvancedPageId,
  type SettingsSectionId,
} from '../pages/settings/navigation';
import { ChannelEditorHostContext } from '../pages/settings/ChannelEditorHostContext';
import { ProviderLeaveContext, useProviderForm } from '../pages/settings/ProviderForm';
import { useContentMotion, useOverlayMotion } from '../lib/motion';

export type { SettingsSectionId } from '../pages/settings/navigation';

interface SettingsModalProps {
  os: OS;
  onClose: () => void;
  initialSettingsSection?: SettingsSectionId;
  /** Keeps the closing surface mounted until its exit finishes. */
  closing?: boolean;
}

const LINKS = [
  { id: 'helpCenter', icon: 'help', href: 'https://github.com/Open-Less/openless#readme' },
  { id: 'releaseNotes', icon: 'doc', href: 'https://github.com/Open-Less/openless/releases' },
];

export function SettingsModal({
  os,
  onClose,
  initialSettingsSection,
  closing = false,
}: SettingsModalProps) {
  const { t, i18n } = useTranslation();
  // The provider form's writes settle before close/section switch (async saves don't
  // lose drafts); the revision counter excludes credential content.
  const providerForm = useProviderForm();
  const mobile = useMobileLayout();
  const conservative = useConservativeLayout();
  const [section, setSection] = useState<SettingsSectionId>(initialSettingsSection ?? 'general');
  const [advancedPage, setAdvancedPage] = useState<AdvancedPageId | null>(null);
  const [query, setQuery] = useState('');
  const [platformCaps, setPlatformCaps] = useState(getCachedPlatformCapabilities);
  const savedToast = useSavedToastListener();
  const surfaceRef = useRef<HTMLDivElement>(null);
  const overlayRef = useRef<HTMLDivElement>(null);
  useOverlayMotion(overlayRef, closing, 'backdrop', !mobile);
  useOverlayMotion(surfaceRef, closing, mobile ? 'sheet' : 'card');
  const scrollRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const mountedRef = useRef(false);
  const [channelContainer, setChannelContainer] = useState<HTMLDivElement | null>(null);
  const [channelBackground, setChannelBackground] = useState<HTMLDivElement | null>(null);
  const channelCloseRef = useRef<(() => void) | null>(null);
  const closeSettings = () => {
    void providerForm.finish(() => {
      channelCloseRef.current?.();
      onClose();
    });
  };
  const supportsShortcuts = platformCaps?.supportsDesktopHotkey ?? os !== 'android';
  const sections = visibleSettingsSections(supportsShortcuts, platformCaps?.platform).map(
    (item) => ({
      ...item,
      title: t(`modal.sections.${item.id}`),
      description: t(`modal.descriptions.${item.id}`),
      keywords: t(`modal.searchKeywords.${item.id}`),
    }),
  );
  const searching = query.trim().length > 0;
  useContentMotion(scrollRef, `${section}:${searching ? 'search' : (advancedPage ?? 'root')}`);
  const results = searchSettingsSections(sections, query);
  const advancedPages = visibleAdvancedPages(platformCaps?.platform, os);
  const activeAdvancedPage =
    section === 'advanced' && !searching
      ? advancedPages.find((page) => page.id === advancedPage)
      : undefined;
  const contentTitle = searching
    ? t('modal.searchResults')
    : activeAdvancedPage
      ? t(activeAdvancedPage.titleKey)
      : t(`modal.sections.${section}`);
  const contentDescription = searching
    ? t('modal.searchCount', { count: results.length })
    : activeAdvancedPage
      ? t(`modal.advancedPages.${activeAdvancedPage.id}`)
      : t(`modal.descriptions.${section}`);

  // The indicator thumb moves to the selected nav row's layout position; hidden
  // while searching or in mobile layout.
  const railNavRef = useRef<HTMLElement>(null);
  const railBtnRefs = useRef(new Map<SettingsSectionId, HTMLButtonElement>());
  const [railThumb, setRailThumb] = useState<{ top: number; height: number } | null>(null);
  useLayoutEffect(() => {
    if (mobile || searching) {
      setRailThumb(null);
      return;
    }
    const nav = railNavRef.current;
    const btn = railBtnRefs.current.get(section);
    if (!nav || !btn) {
      setRailThumb(null);
      return;
    }
    const measure = () => {
      const next = { top: btn.offsetTop, height: btn.offsetHeight };
      setRailThumb((current) =>
        current?.top === next.top && current.height === next.height ? current : next,
      );
    };
    measure();
    // Translated labels may wrap when the language, font scale or window width changes.
    const observer = new ResizeObserver(measure);
    observer.observe(nav);
    observer.observe(btn);
    return () => observer.disconnect();
  }, [section, searching, mobile, sections.length, i18n.resolvedLanguage]);

  useEffect(() => {
    let cancelled = false;
    void getPlatformCapabilities().then((caps) => {
      if (!cancelled) setPlatformCaps(caps);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!supportsShortcuts && section === 'shortcuts') setSection('general');
  }, [supportsShortcuts, section]);

  useEffect(() => {
    if (platformCaps && platformCaps.platform !== 'android' && section === 'inputMethod') {
      setSection('general');
    }
  }, [platformCaps, section]);

  useEffect(() => {
    mountedRef.current = true;
    if (!previousFocusRef.current && document.activeElement instanceof HTMLElement) {
      previousFocusRef.current = document.activeElement;
    }
    // Do not summon a software keyboard when opening mobile settings.
    (mobile ? closeRef.current : searchRef.current)?.focus({ preventScroll: true });
    return () => {
      mountedRef.current = false;
      window.requestAnimationFrame(() => {
        if (!mountedRef.current && previousFocusRef.current?.isConnected)
          previousFocusRef.current.focus();
      });
    };
  }, []);

  useEffect(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = 0;
  }, [section, searching, advancedPage]);

  useEffect(() => {
    if (advancedPage && !advancedPages.some((page) => page.id === advancedPage))
      setAdvancedPage(null);
  }, [advancedPage, platformCaps, os]);

  const openAdvancedPage = (page: AdvancedPageId) => {
    setAdvancedPage(page);
    window.requestAnimationFrame(() => headingRef.current?.focus());
  };

  const backToAdvanced = () => {
    const previousPage = advancedPage;
    setAdvancedPage(null);
    window.requestAnimationFrame(() => {
      surfaceRef.current
        ?.querySelector<HTMLElement>(`[data-ol-advanced-entry="${previousPage}"]`)
        ?.focus();
    });
  };

  const selectSection = (next: SettingsSectionId, fromSearch = false) => {
    void providerForm.finish(() => {
      channelCloseRef.current?.();
      setSection(next);
      setAdvancedPage(null);
      setQuery('');
      if (fromSearch) window.requestAnimationFrame(() => headingRef.current?.focus());
    });
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    // Nested selectors and shortcut recorders own their keys before the dialog does.
    if (event.defaultPrevented || event.nativeEvent.isComposing) return;
    const nestedPopup = document.querySelector('[role="listbox"], [data-base-ui-portal]');
    const target = event.target instanceof Element ? event.target : null;
    if (nestedPopup || target?.closest('[role="dialog"]') !== surfaceRef.current) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      if (query) {
        setQuery('');
        searchRef.current?.focus();
      } else if (activeAdvancedPage) backToAdvanced();
      else closeSettings();
    }
    if (event.key === 'Tab') {
      const focusable = Array.from(
        surfaceRef.current?.querySelectorAll<HTMLElement>(
          'button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex="0"]',
        ) ?? [],
      ).filter((element) => element.getClientRects().length > 0);
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (
        event.shiftKey &&
        (document.activeElement === first || document.activeElement === headingRef.current)
      ) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first?.focus();
      }
    }
  };

  // Search box: desktop puts it at the top of the sidebar (mirroring macOS System
  // Settings' "search above the nav" layout); mobile keeps it full-width under the
  // title bar.
  const searchBox = (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        padding: '7px 10px',
        borderRadius: 8,
        border: '0.5px solid var(--ol-line-strong)',
        background: 'var(--ol-surface)',
        minWidth: 0,
      }}
    >
      <Icon name="search" size={15} />
      <input
        ref={searchRef}
        type="search"
        value={query}
        onChange={(event) => {
          channelCloseRef.current?.();
          setQuery(event.target.value);
        }}
        placeholder={t('modal.searchPlaceholder')}
        aria-label={t('modal.searchPlaceholder')}
        style={{
          width: '100%',
          minWidth: 0,
          border: 0,
          outline: 'none',
          background: 'transparent',
          color: 'var(--ol-ink)',
          font: 'inherit',
          fontSize: 14,
        }}
      />
      {query && (
        <button
          type="button"
          aria-label={t('modal.clearSearch')}
          onClick={() => {
            setQuery('');
            searchRef.current?.focus();
          }}
          style={{ ...iconButtonStyle, width: 22, height: 22 }}
        >
          <Icon name="close" size={12} />
        </button>
      )}
    </div>
  );

  return (
    <ProviderLeaveContext.Provider value={providerForm.register}>
      <div
        ref={overlayRef}
        className={mobile ? undefined : 'ol-dialog-overlay'}
        onClick={mobile || closing ? undefined : closeSettings}
        style={{
          position: mobile ? 'fixed' : 'absolute',
          inset: 0,
          background: mobile ? 'var(--ol-surface)' : 'var(--ol-dialog-backdrop)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          padding: mobile ? 0 : '64px 28px 24px',
          zIndex: mobile ? 70 : 50,
          pointerEvents: closing ? 'none' : undefined,
        }}
      >
        <div
          ref={surfaceRef}
          role="dialog"
          // Existing menus and child dialogs portal to document.body. Keep those
          // accessible; FloatingShell makes the covered application inert.
          aria-label={t('shell.footer.settings')}
          className="ol-settings-surface ol-dialog-card"
          data-ol-mobile={mobile ? 'true' : undefined}
          onClick={(event) => event.stopPropagation()}
          onKeyDown={handleKeyDown}
          style={{
            width: '100%',
            maxWidth: mobile ? undefined : 960,
            height: '100%',
            maxHeight: mobile ? undefined : 680,
            minHeight: 0,
            background: 'var(--ol-settings-content-bg)',
            borderRadius: mobile ? 0 : 'var(--ol-dialog-radius)',
            border: mobile ? 'none' : '1px solid var(--ol-dialog-border)',
            boxShadow: mobile ? 'none' : 'var(--ol-dialog-shadow)',
            display: 'flex',
            flexDirection: 'column',
            overflow: 'hidden',
          }}
        >
          {/* Desktop no longer has a title bar spanning both columns;
            the sidebar and content each run to the top, with title/auto-save/close
            folded into the right column's top. Mobile keeps the full-width header
            (title + close + full-row search). */}
          {mobile && (
            <header
              style={{
                display: 'flex',
                alignItems: 'center',
                flexWrap: 'wrap',
                gap: 12,
                padding: 'calc(12px + env(safe-area-inset-top, 0px)) 16px 12px',
                borderBottom: '0.5px solid var(--ol-line-soft)',
                flexShrink: 0,
              }}
            >
              <div style={{ flex: 1, order: 0, fontSize: 17, fontWeight: 650 }}>
                {t('shell.footer.settings')}
              </div>
              <button
                ref={closeRef}
                type="button"
                onClick={closeSettings}
                aria-label={t('common.close')}
                style={{ ...iconButtonStyle, order: 1 }}
              >
                <Icon name="close" size={17} />
              </button>
              <div style={{ order: 2, flex: '1 0 100%', minWidth: 0 }}>{searchBox}</div>
            </header>
          )}
          <div
            style={{
              flex: 1,
              minHeight: 0,
              display: 'flex',
              flexDirection: mobile ? 'column' : 'row',
            }}
          >
            <aside
              style={{
                width: mobile ? undefined : 214,
                flexShrink: 0,
                minHeight: 0,
                overflow: 'auto',
                padding: mobile ? '8px 12px' : '16px 12px',
                background: 'var(--ol-settings-rail-bg)',
                borderRight: mobile ? undefined : '0.5px solid var(--ol-line-soft)',
                borderBottom: mobile ? '0.5px solid var(--ol-line-soft)' : undefined,
                display: 'flex',
                flexDirection: 'column',
                gap: 12,
              }}
            >
              {!mobile && searchBox}
              <nav
                ref={railNavRef}
                aria-label={t('modal.categoriesLabel')}
                className="ol-thinscroll"
                style={{
                  position: 'relative',
                  display: 'flex',
                  flexDirection: mobile ? 'row' : 'column',
                  gap: 4,
                  overflowX: mobile ? 'auto' : undefined,
                }}
              >
                {/* Sliding blue thumb: top/height follow the active category button, spring-eased. */}
                {!mobile && railThumb && (
                  <div
                    aria-hidden="true"
                    className="ol-settings-rail-thumb"
                    style={{
                      position: 'absolute',
                      left: 0,
                      right: 0,
                      top: 0,
                      transform: `translate3d(0, ${railThumb.top}px, 0)`,
                      height: railThumb.height,
                      borderRadius: 8,
                      background: 'var(--ol-blue-soft)',
                      pointerEvents: 'none',
                    }}
                  />
                )}
                {sections.map((item) => {
                  const active = !searching && section === item.id;
                  return (
                    <button
                      key={item.id}
                      type="button"
                      aria-current={active ? 'page' : undefined}
                      ref={(el) => {
                        if (el) railBtnRefs.current.set(item.id, el);
                        else railBtnRefs.current.delete(item.id);
                      }}
                      onClick={() => selectSection(item.id)}
                      className={
                        mobile
                          ? active
                            ? 'ol-nav-btn ol-nav-btn-active'
                            : 'ol-nav-btn'
                          : 'ol-settings-rail-btn'
                      }
                      style={
                        mobile
                          ? {
                              ...navButtonStyle,
                              flexShrink: 0,
                              padding: '8px 10px',
                              whiteSpace: 'nowrap',
                              background: active ? 'var(--ol-blue-soft)' : 'transparent',
                              color: active ? 'var(--ol-blue)' : 'var(--ol-ink-2)',
                              fontWeight: active ? 600 : 400,
                            }
                          : {
                              ...navButtonStyle,
                              position: 'relative',
                              zIndex: 1,
                              flexShrink: 0,
                              padding: '10px',
                              whiteSpace: 'normal',
                            }
                      }
                    >
                      {!mobile && <Icon name={item.icon} size={16} />}
                      <span style={{ minWidth: 0, overflowWrap: mobile ? undefined : 'anywhere' }}>
                        {item.title}
                      </span>
                    </button>
                  );
                })}
              </nav>
              {!mobile && <HelpLinks />}
            </aside>
            <div
              ref={setChannelContainer}
              className="ol-settings-content-pane"
              data-ol-advanced={section === 'advanced' && !searching ? 'true' : undefined}
              style={{
                flex: 1,
                minWidth: 0,
                minHeight: 0,
                display: 'flex',
                flexDirection: 'column',
                position: 'relative',
              }}
            >
              <ChannelEditorHostContext.Provider
                value={{
                  container: channelContainer,
                  background: channelBackground,
                  registerClose: (close) => {
                    channelCloseRef.current = close;
                  },
                }}
              >
                <div
                  ref={setChannelBackground}
                  style={{ display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0 }}
                >
                  <SavedToast
                    saveState={savedToast.state}
                    message={savedToast.message}
                    slideFrom="top"
                    offsetStyle={{ position: 'absolute', top: 12, right: 16 }}
                  />
                  {/* Desktop: the category title + auto-save hint + close button form
                the right column's own top bar (like System Settings: the toolbar
                belongs to the content area, no longer spanning the sidebar). */}
                  {(!mobile || activeAdvancedPage) && (
                    <div
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        gap: 12,
                        padding: '18px 24px 0',
                        flexShrink: 0,
                      }}
                    >
                      {activeAdvancedPage && (
                        <button
                          type="button"
                          className="ol-settings-back"
                          onClick={backToAdvanced}
                          aria-label={t('modal.backToAdvanced')}
                          title={t('modal.backToAdvanced')}
                        >
                          <Icon name="chevLeft" size={19} />
                        </button>
                      )}
                      <h2
                        ref={headingRef}
                        tabIndex={-1}
                        style={{
                          margin: 0,
                          flex: 1,
                          minWidth: 0,
                          fontSize: 21,
                          fontWeight: 650,
                          letterSpacing: '-0.025em',
                          outline: 'none',
                          overflow: 'hidden',
                          textOverflow: 'ellipsis',
                          whiteSpace: 'nowrap',
                        }}
                      >
                        {contentTitle}
                      </h2>
                      {!mobile && (
                        <>
                          <span style={{ color: 'var(--ol-ink-3)', fontSize: 12, flexShrink: 0 }}>
                            {t('modal.autoSaveHint')}
                          </span>
                          <button
                            ref={closeRef}
                            type="button"
                            onClick={closeSettings}
                            aria-label={t('common.close')}
                            style={iconButtonStyle}
                          >
                            <Icon name="close" size={17} />
                          </button>
                        </>
                      )}
                    </div>
                  )}
                  <div
                    style={{ padding: mobile ? '18px 16px 12px' : '8px 28px 12px', flexShrink: 0 }}
                  >
                    {mobile && !activeAdvancedPage && (
                      <h2
                        ref={headingRef}
                        tabIndex={-1}
                        style={{
                          margin: 0,
                          fontSize: 20,
                          fontWeight: 650,
                          letterSpacing: '-0.025em',
                          outline: 'none',
                        }}
                      >
                        {contentTitle}
                      </h2>
                    )}
                    <p
                      style={{
                        margin: mobile && !activeAdvancedPage ? '7px 0 0' : 0,
                        fontSize: 13,
                        lineHeight: 1.6,
                        color: 'var(--ol-ink-3)',
                      }}
                      aria-live="polite"
                    >
                      {contentDescription}
                    </p>
                  </div>
                  <div
                    ref={scrollRef}
                    className={['ol-thinscroll', conservative ? 'ol-conservative-scope' : ''].join(
                      ' ',
                    )}
                    style={{
                      flex: 1,
                      minHeight: 0,
                      overflow: 'auto',
                      padding: mobile
                        ? '0 16px calc(20px + env(safe-area-inset-bottom, 0px))'
                        : section === 'services'
                          ? '0 28px 6px'
                          : '0 28px 28px',
                    }}
                  >
                    {searching && (
                      <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                        {results.map((item) => (
                          <button
                            key={item.id}
                            type="button"
                            onClick={() => selectSection(item.id, true)}
                            style={{
                              ...navButtonStyle,
                              border: '0.5px solid var(--ol-line)',
                              padding: 16,
                              alignItems: 'flex-start',
                              background: 'var(--ol-surface)',
                            }}
                          >
                            <Icon name={item.icon} size={18} />
                            <span style={{ flex: 1, minWidth: 0 }}>
                              <span style={{ display: 'block', fontWeight: 600, marginBottom: 5 }}>
                                {item.title}
                              </span>
                              <span
                                style={{ fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.6 }}
                              >
                                {item.description}
                              </span>
                            </span>
                            <Icon name="chevRight" size={14} />
                          </button>
                        ))}
                        {results.length === 0 && (
                          <div
                            style={{
                              padding: '24px 16px',
                              border: '0.5px solid var(--ol-line)',
                              borderRadius: 10,
                              textAlign: 'center',
                            }}
                          >
                            <p style={{ fontSize: 13, color: 'var(--ol-ink-3)' }}>
                              {t('modal.noResults')}
                            </p>
                            <button
                              type="button"
                              onClick={() => {
                                setQuery('');
                                searchRef.current?.focus();
                              }}
                              style={{
                                ...navButtonStyle,
                                margin: '12px auto 0',
                                color: 'var(--ol-blue)',
                              }}
                            >
                              {t('modal.clearSearch')}
                            </button>
                          </div>
                        )}
                      </div>
                    )}
                    {/* Category replacement keeps each form's lifecycle separate. */}
                    <div
                      key={section}
                      style={{
                        display: searching ? 'none' : 'flex',
                        flexDirection: 'column',
                        gap: 16,
                      }}
                    >
                      {section === 'general' && <GeneralTab />}
                      {section === 'inputMethod' && <InputMethodTab />}
                      {section === 'shortcuts' && <ShortcutsTab />}
                      {section === 'appearance' && <AppearanceTab />}
                      {section === 'services' && <ServicesTab />}
                      {section === 'privacy' && <PrivacyTab />}
                      {section === 'advanced' && (
                        <AdvancedTab
                          pages={advancedPages}
                          page={advancedPage}
                          onOpenPage={openAdvancedPage}
                        />
                      )}
                      {section === 'about' && <AboutTab />}
                    </div>
                    {mobile && !searching && <HelpLinks />}
                  </div>
                </div>
              </ChannelEditorHostContext.Provider>
            </div>
          </div>
        </div>
      </div>
    </ProviderLeaveContext.Provider>
  );
}

function HelpLinks() {
  const { t } = useTranslation();
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 4,
        paddingTop: 16,
        marginTop: 20,
        borderTop: '0.5px solid var(--ol-line-soft)',
      }}
    >
      {LINKS.map((link) => (
        <button
          key={link.id}
          type="button"
          onClick={() => void openExternal(link.href)}
          className="ol-nav-btn"
          style={navButtonStyle}
        >
          <Icon name={link.icon} size={14} />
          <span style={{ flex: 1 }}>{t(`modal.sections.${link.id}`)}</span>
          <Icon name="external" size={11} />
        </button>
      ))}
    </div>
  );
}

const iconButtonStyle: CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  flexShrink: 0,
  width: 30,
  height: 30,
  padding: 0,
  border: 0,
  borderRadius: 8,
  background: 'transparent',
  color: 'var(--ol-ink-3)',
  cursor: 'pointer',
};
const navButtonStyle: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 10,
  padding: '8px 10px',
  border: 0,
  borderRadius: 8,
  background: 'transparent',
  color: 'var(--ol-ink-2)',
  font: 'inherit',
  fontSize: 14,
  cursor: 'pointer',
  textAlign: 'left',
};
