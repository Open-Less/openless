import { useEffect, useLayoutEffect, useRef, useState, type MutableRefObject } from 'react';
import { useTranslation } from 'react-i18next';
import type { OS } from '../../../runtime/environment';
import { getCachedPlatformCapabilities, getPlatformCapabilities } from '../../../runtime/platform';
import {
  searchSettingsSections,
  visibleAdvancedPages,
  visibleGeneralPages,
  visibleSettingsSections,
  type AdvancedPageId,
  type GeneralPageId,
  type ServiceViewId,
  type SettingsSectionId,
} from './navigation';

export function useSettingsNavigation({
  os,
  initialSettingsSection,
  initialServiceView,
  channelCloseRef,
  beforeLeave,
  onClose,
}: {
  os: OS;
  initialSettingsSection?: SettingsSectionId;
  initialServiceView?: ServiceViewId;
  channelCloseRef: MutableRefObject<(() => void) | null>;
  beforeLeave: (action: () => void | Promise<void>) => Promise<boolean>;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [section, setSection] = useState(initialSettingsSection ?? 'general');
  const [generalPage, setGeneralPage] = useState<GeneralPageId | 'root'>('root');
  const [advancedPage, setAdvancedPage] = useState<AdvancedPageId | null>(null);
  const [voiceSettingsOpen, setVoiceSettingsOpen] = useState(false);
  const [requestedServiceView, setRequestedServiceView] = useState(initialServiceView);
  const [asrNavigation, setAsrNavigation] = useState(0);
  const [query, setQuery] = useState('');
  const [platformCaps, setPlatformCaps] = useState(getCachedPlatformCapabilities);
  const searchRef = useRef<HTMLInputElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const surfaceRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const pendingQuery = useRef('');
  const returnEntry = useRef<GeneralPageId | AdvancedPageId | null>(null);
  const focusHeading = useRef(false);
  const supportsShortcuts = platformCaps?.supportsDesktopHotkey ?? (os === 'mac' || os === 'win');
  const sections = visibleSettingsSections(supportsShortcuts, platformCaps?.platform).map(
    (item) => ({
      ...item,
      title: t(`modal.sections.${item.id}`),
      description: t(`modal.descriptions.${item.id}`),
      keywords: t(`modal.searchKeywords.${item.id}`),
    }),
  );
  const searching = query.trim().length > 0;
  const results = searchSettingsSections(sections, query);
  const advancedPages = visibleAdvancedPages(platformCaps?.platform, os);
  const activeAdvancedPage =
    section === 'advanced' && !searching
      ? advancedPages.find((page) => page.id === advancedPage)
      : undefined;
  const activeGeneralPage =
    section === 'general' && !searching
      ? visibleGeneralPages(platformCaps?.platform).find((page) => page.id === generalPage)
      : undefined;
  const showingVoiceSettings = activeAdvancedPage?.id === 'voiceEnhancement' && voiceSettingsOpen;
  const contentTitle = searching
    ? t('modal.searchResults')
    : showingVoiceSettings
      ? t('settings.recording.dsp.title')
      : activeAdvancedPage
        ? t(activeAdvancedPage.titleKey)
        : activeGeneralPage
          ? t(activeGeneralPage.titleKey)
          : t(`modal.sections.${section}`);
  const contentDescription = searching
    ? t('modal.searchCount', { count: results.length })
    : showingVoiceSettings
      ? t('settings.recording.dsp.description')
      : activeAdvancedPage
        ? t(`modal.advancedPages.${activeAdvancedPage.id}`)
        : activeGeneralPage
          ? t(activeGeneralPage.descriptionKey)
          : t(`modal.descriptions.${section}`);

  useEffect(() => {
    let active = true;
    void getPlatformCapabilities()
      .then((caps) => {
        if (active) setPlatformCaps(caps);
      })
      .catch((error) => {
        if (active) console.warn('[settings] capability read failed', error);
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (!supportsShortcuts && section === 'shortcuts') setSection('general');
    if (platformCaps && platformCaps.platform !== 'android' && section === 'inputMethod')
      setSection('general');
  }, [supportsShortcuts, platformCaps, section]);

  useLayoutEffect(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = 0;
  }, [section, searching, advancedPage, generalPage, voiceSettingsOpen]);

  useEffect(() => {
    if (advancedPage !== 'voiceEnhancement' && voiceSettingsOpen) setVoiceSettingsOpen(false);
    if (
      advancedPage &&
      !visibleAdvancedPages(platformCaps?.platform, os).some((page) => page.id === advancedPage)
    )
      setAdvancedPage(null);
  }, [advancedPage, voiceSettingsOpen, platformCaps, os]);

  const closeSettings = () => {
    void beforeLeave(() => {
      channelCloseRef.current?.();
      onClose();
    });
  };
  const changeQuery = (next: string) => {
    focusHeading.current = false;
    if (!channelCloseRef.current && (section !== 'services' || searching)) {
      setQuery(next);
      return;
    }
    // The latest search text is kept while a previously edited channel flushes.
    pendingQuery.current = next;
    void beforeLeave(() => {
      channelCloseRef.current?.();
      setQuery(pendingQuery.current);
    });
  };
  const openAdvancedPage = (page: AdvancedPageId) => {
    returnEntry.current = null;
    focusHeading.current = true;
    setAdvancedPage(page);
  };
  const backToAdvanced = () => {
    focusHeading.current = false;
    if (advancedPage === 'voiceEnhancement' && voiceSettingsOpen) {
      focusHeading.current = true;
      setVoiceSettingsOpen(false);
      return;
    }
    void beforeLeave(() => {
      returnEntry.current = advancedPage;
      setAdvancedPage(null);
    });
  };
  const onGeneralPageChange = (page: GeneralPageId | 'root') => {
    if (page === 'root') {
      focusHeading.current = false;
      returnEntry.current = activeGeneralPage?.id ?? null;
    } else {
      returnEntry.current = null;
      focusHeading.current = true;
    }
    setGeneralPage(page);
  };
  const focusReturnedEntry = () => {
    if (focusHeading.current) {
      // The heading belongs to the incoming surface and cannot receive focus while inert.
      const heading = surfaceRef.current?.querySelector<HTMLElement>(
        '[data-transition-phase="settled"] h2',
      );
      if (heading && !heading.closest('[inert]')) {
        heading.focus({ preventScroll: true });
        if (document.activeElement === heading) focusHeading.current = false;
      }
      return;
    }
    const id = returnEntry.current;
    if (!id) return;
    const attribute = section === 'general' ? 'data-ol-general-entry' : 'data-ol-advanced-entry';
    const entry = scrollRef.current?.querySelector<HTMLElement>(`[${attribute}="${id}"]`);
    if (entry && !entry.closest('[inert]')) {
      entry.focus();
      if (document.activeElement === entry) returnEntry.current = null;
    }
  };
  const selectSection = (
    next: SettingsSectionId,
    fromSearch = false,
    serviceView?: ServiceViewId,
  ) => {
    void beforeLeave(() => {
      channelCloseRef.current?.();
      setSection(next);
      setRequestedServiceView(next === 'services' ? serviceView : undefined);
      setAdvancedPage(null);
      setGeneralPage('root');
      returnEntry.current = null;
      focusHeading.current = fromSearch;
      setQuery('');
    });
  };
  const jumpToAsr = () => {
    setAsrNavigation((value) => value + 1);
    setQuery('');
    setAdvancedPage(null);
    setGeneralPage('root');
    returnEntry.current = null;
    focusHeading.current = false;
    setSection('services');
  };
  const onEscape = () => {
    if (query) {
      focusHeading.current = false;
      setQuery('');
      searchRef.current?.focus();
    } else if (activeAdvancedPage) backToAdvanced();
    else if (activeGeneralPage) onGeneralPageChange('root');
    else closeSettings();
  };

  return {
    advancedPage,
    advancedPages,
    activeAdvancedPage,
    activeGeneralPage,
    onGeneralPageChange,
    focusReturnedEntry,
    asrNavigation,
    backToAdvanced,
    closeSettings,
    closeRef,
    contentDescription,
    contentTitle,
    destination: `${section}:${activeAdvancedPage?.id ?? activeGeneralPage?.id ?? ''}`,
    jumpToAsr,
    onEscape,
    openAdvancedPage,
    openLocalModels: () => selectSection('services', false, 'models'),
    platformCaps,
    query,
    requestedServiceView,
    results,
    searchRef,
    searching,
    section,
    sections,
    selectSection,
    setQuery,
    changeQuery,
    showingVoiceSettings,
    scrollRef,
    surfaceRef,
    voiceSettingsOpen,
    setVoiceSettingsOpen: (open: boolean) => {
      focusHeading.current = true;
      setVoiceSettingsOpen(open);
    },
  };
}
