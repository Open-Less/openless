// LocalAsr.tsx — local ASR model management page.
//
// Features:
//  - Top: currently active model + mirror source switch
//  - Model list: each row = real size / progress / [download|cancel|delete|set default]
//  - Real size is fetched live from the selected model source via fetchLocalAsrRemoteInfo, never hardcoded
//  - Listens to the `local-asr-download-progress` event to refresh progress in real time
//  - Download button disabled when the engine is unavailable on Windows; see issue #256

import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { LocalModelMetadataCache } from '../../lib/localModelMetadataCache';
import type { LocalAsrRemoteInfo, SherpaOnnxRemoteInfo } from '../../lib/localAsr';
import { restartApp } from '../../lib/ipc/permissions';
import { isTauri } from '../../lib/ipc';
import { emitSaved } from '../../lib/savedEvent';
import { useLayoutStack } from '../../lib/useMobileLayout';
import {
  FOUNDRY_LOCAL_ASR_MODELS,
  LOCAL_ASR_KEEP_LOADED_OPTIONS,
  SHERPA_ONNX_ASR_MODELS,
  activateLocalAsr,
  cancelFoundryLocalAsrPrepare,
  cancelSherpaOnnxAsrDownload,
  cancelSherpaOnnxAsrPrepare,
  cancelLocalAsrDownload,
  cleanupIncompleteLocalAsrModel,
  deleteFoundryLocalAsrModel,
  deleteSherpaOnnxAsrModel,
  deleteLocalAsrModel,
  downloadLocalAsrModel,
  downloadSherpaOnnxAsrModel,
  fetchLocalAsrHfCard,
  fetchLocalAsrRemoteInfo,
  fetchSherpaOnnxAsrRemoteInfo,
  getFoundryLocalAsrModelDir,
  getFoundryLocalAsrCatalog,
  getFoundryLocalAsrStatus,
  getLocalAsrEngineStatus,
  getLocalAsrSettings,
  getSherpaOnnxAsrCatalog,
  getSherpaOnnxAsrModelDir,
  getSherpaOnnxAsrStatus,
  listLocalAsrModels,
  preloadLocalAsr,
  releaseFoundryLocalAsr,
  releaseLocalAsrEngine,
  releaseSherpaOnnxAsr,
  revealFoundryLocalAsrModelDir,
  revealLocalAsrModelDir,
  revealLocalAsrModelsRoot,
  revealSherpaOnnxAsrModelDir,
  setLocalAsrModelsBaseDir,
  setFoundryLocalAsrLanguageHint,
  setFoundryLocalAsrKeepLoadedSecs,
  setFoundryLocalRuntimeSource,
  setLocalAsrKeepLoadedSecs,
  setLocalAsrMirror,
  setSherpaOnnxAsrLanguageHint,
  testLocalAsrModel,
  type FoundryLocalAsrCatalogModel,
  type FoundryLocalAsrLanguageHint,
  type FoundryLocalAsrModelAlias,
  type FoundryLocalAsrStatus,
  type FoundryRuntimeSource,
  type FoundryPrepareProgress,
  type HfModelCard,
  type LocalAsrDownloadProgress,
  type LocalAsrEngineStatus,
  type LocalAsrMirror,
  type LocalAsrModelStatus,
  type LocalAsrSettings,
  type LocalAsrTestResult,
  type SherpaOnnxAsrStatus,
  type SherpaOnnxCatalogModel,
  type SherpaOnnxLanguageHint,
  type SherpaOnnxModelAlias,
  type SherpaPrepareProgress,
  isLocalAsrModelSupportedOnOs,
} from '../../lib/localAsr';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { detectOS } from '../../components/WindowChrome';
import { getPlatformCapabilities } from '../../lib/platform';
import { SelectLite } from '../../components/ui/SelectLite';
import { Icon } from '../../components/Icon';
import { Btn, Card, Collapsible, PageHeader, Pill } from '../_atoms';
import {
  formatBytes,
  formatFoundrySizeMb,
  isFoundryAlias,
  isSherpaAlias,
  isWindowsLikePlatform,
  normalizeFoundryLanguageHintForUi,
  normalizeFoundryRuntimeSourceForUi,
  normalizeSherpaLanguageHintForUi,
} from './helpers';
import {
  DownloadProgressBlock,
  FoundryPrepareProgressBlock,
  ModelDetailPanel,
  ModelSidebar,
  type SidebarModelEntry,
  DownloadDialog,
} from './components';
import type { RemoteSize } from './types';

// The Foundry Local Whisper backend is compiled only on Windows (foundry_local_sdk is Windows-only);
// on other platforms the runtime is a stub that is always unavailable. The cards, status fetches, and
// event subscriptions on this page must be OS-gated so macOS / Linux users never see Windows-only UI.
//
// The MLX build of Qwen3-ASR compiles only on Apple Silicon; C/CPU builds cover macOS / Linux.
// The Qwen3 model management UI stays desktop-gated; the actual backend follows platform
// capabilities and the chosen channel.
const OS = detectOS();
const IS_WINDOWS = OS === 'win';
const IS_QWEN_PLATFORM = OS === 'mac';

function effectiveModelMirror(modelId: string, mirror: string): LocalAsrMirror {
  if (mirror === 'modelscope' && !modelId.startsWith('qwen3-asr-')) return 'huggingface';
  return mirror as LocalAsrMirror;
}

function effectiveSherpaMirror(mirror: string): string {
  return mirror === 'modelscope' ? 'huggingface' : mirror;
}

interface LocalAsrProps {
  /// `embedded=true` renders this as a child of the Advanced settings page (Settings → Advanced);
  /// it then skips the outer page padding/height, PageHeader, and standalone warning Card — the
  /// host AdvancedSection owns those (including unifying the warning into the page-top overlay popup).
  /// `embedded=false` (default) keeps the original full-page style.
  embedded?: boolean;
}

interface LocalAsrContentWrapperProps {
  embedded: boolean;
  children: ReactNode;
}

// Must stay a module-level component: if defined inside the LocalAsr render function, any setState
// from the 3s refresh creates a new component type and React remounts the whole subtree, wiping
// interaction state of Collapsible / SelectLite children.
function LocalAsrContentWrapper({ embedded, children }: LocalAsrContentWrapperProps) {
  if (embedded) return <>{children}</>;
  return (
    <div
      style={{
        padding: '20px 28px 32px',
        overflowY: 'auto',
        height: '100%',
      }}
    >
      {children}
    </div>
  );
}

function LocalAsrGroupTitle({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        fontSize: 12.5,
        fontWeight: 600,
        color: 'var(--ol-ink-3)',
        letterSpacing: '0.02em',
        margin: '18px 0 8px',
      }}
    >
      {children}
    </div>
  );
}

type RefreshGuard = () => boolean;

const remoteInfoCache = new LocalModelMetadataCache<LocalAsrRemoteInfo>();
const sherpaInfoCache = new LocalModelMetadataCache<SherpaOnnxRemoteInfo>();
const modelCardCache = new LocalModelMetadataCache<HfModelCard>();

export function LocalAsr({ embedded = false }: LocalAsrProps = {}) {
  const { t } = useTranslation();
  const keepLoadedOptions = LOCAL_ASR_KEEP_LOADED_OPTIONS.map(({ seconds, labelKey }) => ({
    value: String(seconds),
    label: t(labelKey),
  }));
  const stackLayout = useLayoutStack(1000);
  const { prefs, updatePrefs } = useHotkeySettings();
  const [settings, setSettings] = useState<LocalAsrSettings | null>(null);
  // Wait for the native capability query so Intel Macs never flash the MLX channel first.
  const [supportsQwen3Mlx, setSupportsQwen3Mlx] = useState(false);
  const [models, setModels] = useState<LocalAsrModelStatus[]>([]);
  // Two-pane board: the right side shows the selected model (defaults to the first downloaded one).
  const [selectedModelId, setSelectedModelId] = useState<string | null>(null);
  // Download dialog toggle: opened from the sidebar "download new model" / board "download" actions.
  const [downloadDialogOpen, setDownloadDialogOpen] = useState(false);
  const [progress, setProgress] = useState<Record<string, LocalAsrDownloadProgress>>({});
  const [remoteSizes, setRemoteSizes] = useState<Record<string, RemoteSize>>({});
  // HF model cards (downloads/stars/description) shown on the right of the dialog; successful
  // results are cached, failures record { loading:false, error } to allow retry.
  const [hfCards, setHfCards] = useState<
    Record<string, HfModelCard | { loading: boolean; error: string | null }>
  >({});
  const [error, setError] = useState<string | null>(null);
  const [busyModelId, setBusyModelId] = useState<string | null>(null);
  const [storageBusy, setStorageBusy] = useState(false);
  const [catalogRefreshing, setCatalogRefreshing] = useState(false);
  const [foundryStatus, setFoundryStatus] = useState<FoundryLocalAsrStatus | null>(null);
  const [foundryCatalog, setFoundryCatalog] = useState<FoundryLocalAsrCatalogModel[]>([]);
  const [selectedFoundryAlias, setSelectedFoundryAlias] =
    useState<FoundryLocalAsrModelAlias>('whisper-small');
  const [foundryBusy, setFoundryBusy] = useState<
    'enable' | 'prepare' | 'release' | 'delete' | 'reveal' | null
  >(null);
  const [foundryProgress, setFoundryProgress] = useState<FoundryPrepareProgress | null>(null);
  const [foundryCancelRequested, setFoundryCancelRequested] = useState(false);
  const [foundryModelDir, setFoundryModelDir] = useState<{
    alias: FoundryLocalAsrModelAlias;
    dir: string;
  } | null>(null);
  const [sherpaStatus, setSherpaStatus] = useState<SherpaOnnxAsrStatus | null>(null);
  const [sherpaCatalog, setSherpaCatalog] = useState<SherpaOnnxCatalogModel[]>([]);
  const [selectedSherpaAlias, setSelectedSherpaAlias] =
    useState<SherpaOnnxModelAlias>('sense-voice-small-zh');
  const [sherpaBusy, setSherpaBusy] = useState<
    'enable' | 'prepare' | 'download' | 'release' | 'delete' | 'reveal' | null
  >(null);
  const [sherpaProgress, setSherpaProgress] = useState<SherpaPrepareProgress | null>(null);
  const [sherpaDownloadProgress, setSherpaDownloadProgress] = useState<
    Record<string, LocalAsrDownloadProgress>
  >({});
  const [sherpaRemoteSizes, setSherpaRemoteSizes] = useState<Record<string, RemoteSize>>({});
  const [sherpaCancelRequested, setSherpaCancelRequested] = useState(false);
  const [sherpaDownloadCancelRequested, setSherpaDownloadCancelRequested] = useState(false);
  const [sherpaModelDir, setSherpaModelDir] = useState('');
  const [testingModelId, setTestingModelId] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<
    Record<string, LocalAsrTestResult | { error: string }>
  >({});
  const [engineStatus, setEngineStatus] = useState<LocalAsrEngineStatus | null>(null);
  const catalogReloadRequestedRef = useRef(false);
  const downloadDialogOpenRef = useRef(downloadDialogOpen);
  const refreshGenerationRef = useRef(0);
  const metadataMirrorRef = useRef(settings?.mirror);
  metadataMirrorRef.current = settings?.mirror;
  const makeMetadataGuard = (): RefreshGuard => {
    const generation = refreshGenerationRef.current;
    const mirror = metadataMirrorRef.current;
    return () =>
      generation === refreshGenerationRef.current && mirror === metadataMirrorRef.current;
  };
  const refreshTimer = useRef<number | null>(null);
  const foundryRefreshTimer = useRef<number | null>(null);
  const sherpaRefreshTimer = useRef<number | null>(null);
  const sherpaDownloadRefreshTimer = useRef<number | null>(null);
  const foundrySelectionDirty = useRef(false);
  // Row container shared by the three Foundry SelectLites: SelectLite's onChange has no
  // e.currentTarget, so scroll preservation walks up from this container to find .ol-thinscroll.
  const foundryControlsRef = useRef<HTMLDivElement>(null);
  const selectedFoundryAliasRef = useRef<FoundryLocalAsrModelAlias>('whisper-small');
  const sherpaSelectionDirty = useRef(false);
  const sherpaAnchorRef = useRef<HTMLDivElement>(null);
  const scrollGuard = useRef<{ scroller: HTMLElement; top: number } | null>(null);
  const scrollGuardTimer = useRef<number | null>(null);
  const scrollGuardCleanup = useRef<(() => void) | null>(null);

  useEffect(() => {
    void getPlatformCapabilities().then((caps) => setSupportsQwen3Mlx(caps.supportsLocalQwen3Mlx));
  }, []);

  const setDownloadDialog = (open: boolean) => {
    if (downloadDialogOpenRef.current !== open) {
      downloadDialogOpenRef.current = open;
      refreshGenerationRef.current += 1;
    }
    setDownloadDialogOpen(open);
  };

  // Clearing the interval only stops the next tick; the generation guard also discards in-flight async results.
  const makeRefreshGuard = (): RefreshGuard => {
    const generation = refreshGenerationRef.current;
    return () => generation === refreshGenerationRef.current && !downloadDialogOpenRef.current;
  };

  const restoreScrollGuard = () => {
    const guard = scrollGuard.current;
    if (!guard) return;
    if (guard.scroller.scrollTop !== guard.top) {
      guard.scroller.scrollTop = guard.top;
    }
  };

  const scheduleScrollGuardRestore = () => {
    // issue #470: the immediate frames are covered by the rAF + nested rAF below (≈0-32ms), so the equivalent
    // setTimeout(…,0) was removed; the 80ms / 200ms shots remain to catch async reflows after rAF (e.g. late images).
    window.setTimeout(restoreScrollGuard, 80);
    window.setTimeout(restoreScrollGuard, 200);
    window.requestAnimationFrame(() => {
      restoreScrollGuard();
      window.requestAnimationFrame(restoreScrollGuard);
    });
  };

  const activateScrollGuard = () => {
    if (scrollGuardCleanup.current) scrollGuardCleanup.current();
    const scroller = sherpaAnchorRef.current?.closest('.ol-thinscroll') as HTMLElement | null;
    if (!scroller) return;
    scrollGuard.current = { scroller, top: scroller.scrollTop };
    scheduleScrollGuardRestore();

    const deactivate = () => {
      scrollGuard.current = null;
      scroller.removeEventListener('wheel', deactivate);
      scroller.removeEventListener('pointerdown', deactivate);
      if (scrollGuardTimer.current) {
        window.clearTimeout(scrollGuardTimer.current);
        scrollGuardTimer.current = null;
      }
      scrollGuardCleanup.current = null;
    };
    scrollGuardCleanup.current = deactivate;
    scroller.addEventListener('wheel', deactivate, {
      once: true,
      passive: true,
    });
    scroller.addEventListener('pointerdown', deactivate, { once: true });
    if (scrollGuardTimer.current) window.clearTimeout(scrollGuardTimer.current);
    scrollGuardTimer.current = window.setTimeout(deactivate, 10_000);
  };

  useLayoutEffect(() => {
    restoreScrollGuard();
  });

  const preserveEmbeddedScroll = (element: Element | null) => {
    const scroller = element?.closest('.ol-thinscroll') as HTMLElement | null;
    if (!scroller) return () => undefined;
    const top = scroller.scrollTop;
    return () => {
      window.requestAnimationFrame(() => {
        scroller.scrollTop = top;
      });
    };
  };

  const setCurrentFoundryAlias = (alias: FoundryLocalAsrModelAlias) => {
    if (selectedFoundryAliasRef.current !== alias) {
      setFoundryModelDir(null);
    }
    selectedFoundryAliasRef.current = alias;
    setSelectedFoundryAlias(alias);
  };

  const refreshEngineStatus = async () => {
    const isCurrent = makeRefreshGuard();
    try {
      const status = await getLocalAsrEngineStatus();
      if (!isCurrent()) return;
      setEngineStatus(status);
    } catch (err) {
      console.warn('[localAsr] engine status query failed', err);
    }
  };

  const refreshFoundryStatus = async () => {
    const isCurrent = makeRefreshGuard();
    try {
      const status = await getFoundryLocalAsrStatus();
      if (!isCurrent()) return;
      setFoundryStatus(status);
      if (!foundrySelectionDirty.current && isFoundryAlias(status.activeModel)) {
        setCurrentFoundryAlias(status.activeModel);
        void refreshFoundryModelDir(status.activeModel);
      }
    } catch (err) {
      if (!isCurrent()) return;
      const message = err instanceof Error ? err.message : String(err);
      setFoundryStatus({
        providerId: 'foundry-local-whisper',
        available: false,
        runtimeReady: false,
        runtimeSource: selectedFoundryRuntimeSource,
        activeModel: selectedFoundryAlias,
        loadedModelId: null,
        keepLoadedSecs: 300,
        endpoint: null,
        error: message,
      });
    }
  };

  const refreshFoundryCatalog = async () => {
    const isCurrent = makeRefreshGuard();
    try {
      const catalog = await getFoundryLocalAsrCatalog();
      if (!isCurrent()) return;
      setFoundryCatalog(catalog);
    } catch (err) {
      console.warn('[localAsr] Foundry catalog query failed', err);
    }
  };

  const refreshFoundryModelDir = async (modelAlias: FoundryLocalAsrModelAlias) => {
    const isCurrent = makeRefreshGuard();
    try {
      const dir = await getFoundryLocalAsrModelDir(modelAlias);
      if (!isCurrent()) return;
      setFoundryModelDir((current) => {
        if (selectedFoundryAliasRef.current !== modelAlias) {
          return current;
        }
        if (current?.alias === modelAlias && current.dir === dir) {
          return current;
        }
        return {
          alias: modelAlias,
          dir,
        };
      });
    } catch (err) {
      if (!isCurrent()) return;
      console.warn('[localAsr] Foundry model dir query failed', err);
      setFoundryModelDir((current) =>
        selectedFoundryAliasRef.current === modelAlias && current?.alias === modelAlias
          ? null
          : current,
      );
    }
  };

  const refreshSherpaStatus = async () => {
    const isCurrent = makeRefreshGuard();
    try {
      const status = await getSherpaOnnxAsrStatus();
      if (!isCurrent()) return;
      setSherpaStatus(status);
      if (!sherpaSelectionDirty.current && isSherpaAlias(status.activeModel)) {
        setSelectedSherpaAlias(status.activeModel);
        void refreshSherpaModelDir(status.activeModel);
      }
    } catch (err) {
      if (!isCurrent()) return;
      const message = err instanceof Error ? err.message : String(err);
      setSherpaStatus({
        providerId: 'sherpa-onnx-local',
        available: false,
        runtimeReady: false,
        activeModel: selectedSherpaAlias,
        loadedModelId: null,
        error: message,
      });
    }
  };

  const refreshSherpaCatalog = async () => {
    const isCurrent = makeRefreshGuard();
    try {
      const catalog = await getSherpaOnnxAsrCatalog();
      if (!isCurrent()) return;
      setSherpaCatalog(catalog);
    } catch (err) {
      console.warn('[localAsr] Sherpa catalog query failed', err);
    }
  };

  const refreshSherpaModelDir = async (modelAlias: string) => {
    const isCurrent = makeRefreshGuard();
    try {
      const dir = await getSherpaOnnxAsrModelDir(modelAlias);
      if (!isCurrent()) return;
      setSherpaModelDir((current) => (current === dir ? current : dir));
    } catch (err) {
      console.warn('[localAsr] Sherpa model dir query failed', err);
    }
  };

  const refresh = async () => {
    const isCurrent = makeRefreshGuard();
    try {
      if (!isCurrent()) return;
      setError(null);
      const [s, list] = await Promise.all([getLocalAsrSettings(), listLocalAsrModels()]);
      if (!isCurrent()) return;
      const supportedModels = list.filter(
        (model) => model.runtime === 'generic' && isLocalAsrModelSupportedOnOs(model, OS),
      );
      setSettings(s);
      setModels(supportedModels);
      void refreshEngineStatus();
      if (IS_WINDOWS) {
        void refreshFoundryStatus();
        void refreshFoundryCatalog();
        void refreshFoundryModelDir(selectedFoundryAlias);
        void refreshSherpaStatus();
        void refreshSherpaCatalog();
        void refreshSherpaModelDir(selectedSherpaAlias);
      }
    } catch (e) {
      if (!isCurrent()) return;
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const ensureRemoteSize = async (modelId: string, mirror: string) => {
    const isCurrent = makeMetadataGuard();
    if (!isCurrent()) return;
    setRemoteSizes((prev) => {
      if (prev[modelId] && !prev[modelId].error) return prev;
      return {
        ...prev,
        [modelId]: {
          totalBytes: 0,
          fileCount: 0,
          loading: true,
          error: null,
        },
      };
    });
    try {
      const info = await remoteInfoCache.load(JSON.stringify([modelId, mirror]), () =>
        fetchLocalAsrRemoteInfo(modelId, mirror),
      );
      if (!isCurrent()) return;
      setRemoteSizes((prev) => ({
        ...prev,
        [modelId]: {
          totalBytes: info.totalBytes,
          fileCount: info.files.length,
          loading: false,
          error: null,
        },
      }));
    } catch (e) {
      if (!isCurrent()) return;
      setRemoteSizes((prev) => ({
        ...prev,
        [modelId]: {
          totalBytes: 0,
          fileCount: 0,
          loading: false,
          error: e instanceof Error ? e.message : String(e),
        },
      }));
    }
  };

  // HF model cards are fetched on demand (when a model is selected in the dialog); successful results are cached.
  const ensureHfCard = async (modelId: string, mirror: string) => {
    const isCurrent = makeMetadataGuard();
    setHfCards((prev) => ({
      ...prev,
      [modelId]: { loading: true, error: null },
    }));
    try {
      const card = await modelCardCache.load(JSON.stringify([modelId, mirror]), () =>
        fetchLocalAsrHfCard(modelId, mirror),
      );
      if (!isCurrent()) return;
      setHfCards((prev) => ({ ...prev, [modelId]: card }));
    } catch (e) {
      if (!isCurrent()) return;
      setHfCards((prev) => ({
        ...prev,
        [modelId]: {
          loading: false,
          error: e instanceof Error ? e.message : String(e),
        },
      }));
    }
  };

  const ensureSherpaRemoteSize = async (modelAlias: string, mirror: string) => {
    const isCurrent = makeMetadataGuard();
    if (!isCurrent()) return;
    setSherpaRemoteSizes((prev) => {
      if (prev[modelAlias] && !prev[modelAlias].error) return prev;
      return {
        ...prev,
        [modelAlias]: {
          totalBytes: 0,
          fileCount: 0,
          loading: true,
          error: null,
        },
      };
    });
    try {
      const info = await sherpaInfoCache.load(JSON.stringify([modelAlias, mirror]), () =>
        fetchSherpaOnnxAsrRemoteInfo(modelAlias, mirror),
      );
      if (!isCurrent()) return;
      setSherpaRemoteSizes((prev) => ({
        ...prev,
        [modelAlias]: {
          totalBytes: info.totalBytes,
          fileCount: info.files.length,
          loading: false,
          error: null,
        },
      }));
    } catch (e) {
      if (!isCurrent()) return;
      setSherpaRemoteSizes((prev) => ({
        ...prev,
        [modelAlias]: {
          totalBytes: 0,
          fileCount: 0,
          loading: false,
          error: e instanceof Error ? e.message : String(e),
        },
      }));
    }
  };

  useEffect(() => {
    void refresh();
    return () => {
      if (scrollGuardCleanup.current) scrollGuardCleanup.current();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Pause the 3s poll while the download dialog is open: the dialog is a static catalog picker and poll
  // setStates reshuffle board content behind the translucent mask, which is visibly jumping. Polling
  // restarts automatically when the dialog closes (the effect keyed on downloadDialogOpen rebuilds the interval).
  useEffect(() => {
    if (downloadDialogOpen) return;
    const pollTimer = window.setInterval(() => {
      void refresh();
    }, 3000);
    return () => {
      refreshGenerationRef.current += 1;
      window.clearInterval(pollTimer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [downloadDialogOpen]);

  // Engine status is now emitted by the backend (load/release/keepLoadedSecs changes), so the frontend never polls.
  // Still fetch the initial value on mount, then listen to `local-asr:engine-changed` for updates.
  // Tauri only (the browser dev mock has no events).
  useEffect(() => {
    if (!isTauri) return;
    void refreshEngineStatus();
    let unlisten: undefined | (() => void);
    let cancelled = false;
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const off = await listen<LocalAsrEngineStatus>('local-asr:engine-changed', (e) => {
        setEngineStatus(e.payload);
      });
      if (cancelled) {
        off();
      } else {
        unlisten = off;
      }
    })().catch((err) => console.warn('[localAsr] engine status subscribe failed', err));
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Changing the source invalidates displayed metadata, not the local catalog.
  useEffect(() => {
    setRemoteSizes({});
    setSherpaRemoteSizes({});
    setHfCards({});
  }, [settings?.mirror]);

  // Opening Services is local-only. Fetch remote metadata only for the model
  // selected in the download dialog; concurrent renders share the same request.
  useEffect(() => {
    if (!downloadDialogOpen || !selectedModelId || !settings) return;
    const entry = allSidebarEntries.find((e) => e.id === selectedModelId);
    if (!entry) return;
    if (entry.engine === 'sherpa') {
      void ensureSherpaRemoteSize(entry.id, effectiveSherpaMirror(settings.mirror));
    } else if (entry.engine === 'qwen3' || entry.engine === 'whisper') {
      void ensureRemoteSize(entry.id, effectiveModelMirror(entry.id, settings.mirror));
    }
    if (entry.repo) {
      void ensureHfCard(entry.id, effectiveModelMirror(entry.id, settings.mirror));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [downloadDialogOpen, selectedModelId, settings?.mirror]);

  // Subscribe to download progress events — Tauri only (the browser dev mock has no events).
  useEffect(() => {
    if (!isTauri) return;
    let unlisten: undefined | (() => void);
    let cancelled = false;
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const off = await listen<LocalAsrDownloadProgress>('local-asr-download-progress', (e) => {
        const payload = e.payload;
        if (payload.phase === 'cancelled') {
          // On cancel, drop the entry; whether the bar still shows is decided by hasPartial
          setProgress((prev) => {
            const next = { ...prev };
            delete next[payload.modelId];
            return next;
          });
        } else {
          setProgress((prev) => ({
            ...prev,
            [payload.modelId]: payload,
          }));
        }
        if (
          payload.phase === 'finished' ||
          payload.phase === 'cancelled' ||
          payload.phase === 'failed'
        ) {
          if (refreshTimer.current) window.clearTimeout(refreshTimer.current);
          refreshTimer.current = window.setTimeout(() => {
            void refresh();
          }, 200);
        }
      });
      if (cancelled) {
        off();
      } else {
        unlisten = off;
      }
    })().catch((err) => console.warn('[localAsr] subscribe failed', err));
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
      if (refreshTimer.current) window.clearTimeout(refreshTimer.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!isTauri || !IS_WINDOWS) return;
    let unlisten: undefined | (() => void);
    let cancelled = false;
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const off = await listen<FoundryPrepareProgress>(
        'foundry-local-asr-prepare-progress',
        (e) => {
          const payload = e.payload;
          setFoundryProgress(payload);
          if (payload.phase === 'finished' || payload.phase === 'failed') {
            if (foundryRefreshTimer.current) window.clearTimeout(foundryRefreshTimer.current);
            foundryRefreshTimer.current = window.setTimeout(() => {
              void refreshFoundryStatus();
              void refreshFoundryCatalog();
            }, 200);
          }
        },
      );
      if (cancelled) {
        off();
      } else {
        unlisten = off;
      }
    })().catch((err) => console.warn('[localAsr] Foundry prepare subscribe failed', err));
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
      if (foundryRefreshTimer.current) window.clearTimeout(foundryRefreshTimer.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!isTauri || !IS_WINDOWS) return;
    let unlisten: undefined | (() => void);
    let cancelled = false;
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const off = await listen<SherpaPrepareProgress>('sherpa-onnx-asr-prepare-progress', (e) => {
        const payload = e.payload;
        setSherpaProgress(payload);
        if (payload.phase === 'finished' || payload.phase === 'failed') {
          if (sherpaRefreshTimer.current) window.clearTimeout(sherpaRefreshTimer.current);
          sherpaRefreshTimer.current = window.setTimeout(() => {
            void refreshSherpaStatus();
            void refreshSherpaCatalog();
          }, 200);
        }
      });
      if (cancelled) {
        off();
      } else {
        unlisten = off;
      }
    })().catch((err) => console.warn('[localAsr] Sherpa prepare subscribe failed', err));
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
      if (sherpaRefreshTimer.current) window.clearTimeout(sherpaRefreshTimer.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!isTauri || !IS_WINDOWS) return;
    let unlisten: undefined | (() => void);
    let cancelled = false;
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const off = await listen<LocalAsrDownloadProgress>(
        'sherpa-onnx-asr-download-progress',
        (e) => {
          const payload = e.payload;
          setSherpaDownloadProgress((prev) => ({
            ...prev,
            [payload.modelId]: payload,
          }));
          if (
            payload.phase === 'finished' ||
            payload.phase === 'cancelled' ||
            payload.phase === 'failed'
          ) {
            setSherpaBusy((current) => (current === 'download' ? null : current));
            setSherpaDownloadCancelRequested(false);
            if (sherpaDownloadRefreshTimer.current) {
              window.clearTimeout(sherpaDownloadRefreshTimer.current);
            }
            sherpaDownloadRefreshTimer.current = window.setTimeout(() => {
              void refreshSherpaStatus();
              void refreshSherpaCatalog();
              void refreshSherpaModelDir(payload.modelId);
            }, 200);
          }
        },
      );
      if (cancelled) {
        off();
      } else {
        unlisten = off;
      }
    })().catch((err) => console.warn('[localAsr] Sherpa download subscribe failed', err));
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
      if (sherpaDownloadRefreshTimer.current)
        window.clearTimeout(sherpaDownloadRefreshTimer.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const applyModelsBaseDir = async (modelsBaseDir: string | null) => {
    setStorageBusy(true);
    try {
      setError(null);
      const next = await setLocalAsrModelsBaseDir(modelsBaseDir);
      setSettings((current) =>
        current
          ? {
              ...current,
              modelsBaseDir: next.modelsBaseDir,
              modelsRootDir: next.modelsRootDir,
            }
          : current,
      );
      if (next.restartRequired) {
        await restartApp();
        return;
      }
      await refresh();
      void refreshFoundryModelDir(selectedFoundryAlias);
      void refreshSherpaModelDir(selectedSherpaAlias);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setStorageBusy(false);
    }
  };

  const handleChooseModelsBaseDir = async () => {
    if (!isTauri) {
      await applyModelsBaseDir('~/OpenLessModels');
      return;
    }
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({
      directory: true,
      multiple: false,
      title: t('localAsr.storageChooseTitle'),
    });
    if (!picked || Array.isArray(picked)) return;
    if (
      !window.confirm(
        t('localAsr.storageChangeConfirm', {
          path: picked,
        }),
      )
    ) {
      return;
    }
    await applyModelsBaseDir(picked);
  };

  const handleResetModelsBaseDir = async () => {
    if (
      !window.confirm(
        t('localAsr.storageResetConfirm', {
          path: settings?.modelsRootDir ?? '',
        }),
      )
    ) {
      return;
    }
    await applyModelsBaseDir(null);
  };

  const handleRevealModelsRoot = async () => {
    try {
      setError(null);
      await revealLocalAsrModelsRoot();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleFoundryLanguageChange = async (
    languageHint: FoundryLocalAsrLanguageHint,
    restoreScroll?: () => void,
  ) => {
    try {
      setError(null);
      await setFoundryLocalAsrLanguageHint(languageHint);
      await updatePrefs((current) =>
        current.foundryLocalAsrLanguageHint === languageHint
          ? current
          : {
              ...current,
              foundryLocalAsrLanguageHint: languageHint,
            },
      );
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      restoreScroll?.();
    }
  };

  const handleFoundryRuntimeSourceChange = async (
    runtimeSource: FoundryRuntimeSource,
    restoreScroll?: () => void,
  ) => {
    try {
      setError(null);
      await setFoundryLocalRuntimeSource(runtimeSource);
      await updatePrefs((current) =>
        current.foundryLocalRuntimeSource === runtimeSource
          ? current
          : {
              ...current,
              foundryLocalRuntimeSource: runtimeSource,
            },
      );
      await refreshFoundryStatus();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      restoreScroll?.();
    }
  };

  const handleFoundryKeepLoadedChange = async (seconds: number, restoreScroll?: () => void) => {
    try {
      setError(null);
      await setFoundryLocalAsrKeepLoadedSecs(seconds);
      await refreshFoundryStatus();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      restoreScroll?.();
    }
  };

  const handleEnableFoundry = async (aliasOverride?: FoundryLocalAsrModelAlias) => {
    if (!foundryAvailable) return;
    const alias = aliasOverride ?? selectedFoundryAlias;
    setFoundryBusy('enable');
    try {
      setError(null);
      await activateLocalAsr('foundry', alias, 'foundry-local-whisper');
      foundrySelectionDirty.current = false;
      await refreshFoundryStatus();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setFoundryBusy(null);
    }
  };

  const handlePrepareFoundry = async (aliasOverride?: FoundryLocalAsrModelAlias) => {
    if (!foundryAvailable) return;
    const alias = aliasOverride ?? selectedFoundryAlias;
    setFoundryBusy('prepare');
    setFoundryCancelRequested(false);
    setFoundryProgress({
      phase: 'runtime',
      modelAlias: alias,
      label: t('localAsr.foundryPrepareRuntime'),
      percent: 0,
      error: null,
    });
    try {
      setError(null);
      await activateLocalAsr('foundry', alias, 'foundry-local-whisper');
      foundrySelectionDirty.current = false;
      await refreshFoundryStatus();
      await refreshFoundryCatalog();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      await refreshFoundryStatus();
      await refreshFoundryCatalog();
    } finally {
      setFoundryBusy(null);
      setFoundryCancelRequested(false);
    }
  };

  // Sidebar "download" action: enable first (switch provider + write model), then prepare/download/load in order.
  // Atomic activation already covers prepare/preload; sidebar actions must not chain a second settings write.
  const handleEnableAndPrepareFoundry = async (alias: FoundryLocalAsrModelAlias) => {
    await handleEnableFoundry(alias);
  };

  const handleCancelFoundryPrepare = async () => {
    if (foundryBusy !== 'prepare') return;
    setFoundryCancelRequested(true);
    try {
      await cancelFoundryLocalAsrPrepare();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleReleaseFoundry = async () => {
    setFoundryBusy('release');
    try {
      setError(null);
      await releaseFoundryLocalAsr();
      await refreshFoundryStatus();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setFoundryBusy(null);
    }
  };

  const handleRevealFoundryDir = async () => {
    setFoundryBusy('reveal');
    try {
      setError(null);
      await revealFoundryLocalAsrModelDir(selectedFoundryAlias);
      await refreshFoundryModelDir(selectedFoundryAlias);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setFoundryBusy(null);
    }
  };

  const handleDeleteFoundry = async (aliasOverride?: FoundryLocalAsrModelAlias) => {
    const alias = aliasOverride ?? selectedFoundryAlias;
    const displayName =
      foundryCatalog.find((m) => m.alias === alias)?.displayName ??
      t(
        (FOUNDRY_LOCAL_ASR_MODELS.find((m) => m.alias === alias) ?? FOUNDRY_LOCAL_ASR_MODELS[0])
          .labelKey,
      );
    if (
      !window.confirm(
        t('localAsr.deleteConfirm', {
          name: displayName,
        }),
      )
    ) {
      return;
    }
    setFoundryBusy('delete');
    try {
      setError(null);
      await deleteFoundryLocalAsrModel(alias);
      await refreshFoundryStatus();
      await refreshFoundryCatalog();
      await refreshFoundryModelDir(alias);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setFoundryBusy(null);
    }
  };

  const activateSherpaProvider = async (modelAlias: SherpaOnnxModelAlias) => {
    await activateLocalAsr('sherpa_onnx', modelAlias, 'sherpa-onnx-local');
    sherpaSelectionDirty.current = false;
  };

  const handleSherpaModelChange = async (alias: SherpaOnnxModelAlias) => {
    activateScrollGuard();
    sherpaSelectionDirty.current = true;
    setSelectedSherpaAlias(alias);
    void refreshSherpaModelDir(alias);
    try {
      setError(null);
      await activateSherpaProvider(alias);
      await refreshSherpaStatus();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleSherpaLanguageChange = async (
    languageHint: SherpaOnnxLanguageHint,
    restoreScroll?: () => void,
  ) => {
    try {
      setError(null);
      await setSherpaOnnxAsrLanguageHint(languageHint);
      await updatePrefs((current) =>
        current.sherpaOnnxLanguageHint === languageHint
          ? current
          : {
              ...current,
              sherpaOnnxLanguageHint: languageHint,
            },
      );
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      restoreScroll?.();
    }
  };

  const handleEnableSherpa = async () => {
    if (!sherpaAvailable) return;
    setSherpaBusy('enable');
    try {
      setError(null);
      await activateSherpaProvider(selectedSherpaAlias);
      await refreshSherpaStatus();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSherpaBusy(null);
    }
  };

  const handlePrepareSherpa = async () => {
    if (!sherpaAvailable) return;
    setSherpaBusy('prepare');
    setSherpaCancelRequested(false);
    setSherpaProgress({
      phase: 'model',
      modelAlias: selectedSherpaAlias,
      label: t('localAsr.sherpaPrepareLocalFiles'),
      percent: 0,
      error: null,
    });
    try {
      setError(null);
      await activateSherpaProvider(selectedSherpaAlias);
      sherpaSelectionDirty.current = false;
      await refreshSherpaStatus();
      await refreshSherpaCatalog();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      await refreshSherpaStatus();
      await refreshSherpaCatalog();
    } finally {
      setSherpaBusy(null);
      setSherpaCancelRequested(false);
    }
  };

  const handleCancelSherpaPrepare = async () => {
    if (sherpaBusy !== 'prepare') return;
    setSherpaCancelRequested(true);
    try {
      await cancelSherpaOnnxAsrPrepare();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleReleaseSherpa = async () => {
    setSherpaBusy('release');
    try {
      setError(null);
      await releaseSherpaOnnxAsr();
      await refreshSherpaStatus();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSherpaBusy(null);
    }
  };

  const handleRevealSherpaDir = async () => {
    setSherpaBusy('reveal');
    try {
      setError(null);
      await revealSherpaOnnxAsrModelDir(selectedSherpaAlias);
      await refreshSherpaModelDir(selectedSherpaAlias);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSherpaBusy(null);
    }
  };

  const handleDeleteSherpa = async (aliasOverride?: SherpaOnnxModelAlias) => {
    const alias = aliasOverride ?? selectedSherpaAlias;
    const displayName =
      sherpaCatalog.find((m) => m.alias === alias)?.displayName ??
      t(
        (SHERPA_ONNX_ASR_MODELS.find((m) => m.alias === alias) ?? SHERPA_ONNX_ASR_MODELS[0])
          .labelKey,
      );
    if (
      !window.confirm(
        t('localAsr.deleteConfirm', {
          name: displayName,
        }),
      )
    ) {
      return;
    }
    setSherpaBusy('delete');
    try {
      setError(null);
      await deleteSherpaOnnxAsrModel(alias);
      setSherpaDownloadProgress((prev) => {
        const next = { ...prev };
        delete next[alias];
        return next;
      });
      await refreshSherpaStatus();
      await refreshSherpaCatalog();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSherpaBusy(null);
    }
  };

  const handleDownloadSherpa = async (aliasOverride?: SherpaOnnxModelAlias) => {
    if (!sherpaAvailable) return;
    const modelAlias = aliasOverride ?? selectedSherpaAlias;
    const remoteSize = sherpaRemoteSizes[modelAlias];
    const model = sherpaCatalog.find((item) => item.alias === modelAlias);
    const initialDownloaded =
      sherpaDownloadProgress[modelAlias]?.bytesDownloaded ?? model?.downloadedBytes ?? 0;
    setSherpaBusy('download');
    setSherpaDownloadCancelRequested(false);
    setSherpaDownloadProgress((prev) => ({
      ...prev,
      [modelAlias]: {
        modelId: modelAlias,
        file: '',
        fileIndex: 0,
        fileCount: remoteSize?.fileCount ?? 0,
        bytesDownloaded: initialDownloaded,
        bytesTotal: remoteSize?.totalBytes ?? 0,
        phase: 'started',
        error: null,
      },
    }));
    try {
      setError(null);
      await downloadSherpaOnnxAsrModel(
        modelAlias,
        effectiveSherpaMirror(settings?.mirror ?? 'huggingface'),
      );
      await activateSherpaProvider(modelAlias);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
      setSherpaDownloadProgress((prev) => {
        const cur = prev[modelAlias];
        return {
          ...prev,
          [modelAlias]: {
            modelId: modelAlias,
            file: cur?.file ?? '',
            fileIndex: cur?.fileIndex ?? 0,
            fileCount: cur?.fileCount ?? remoteSize?.fileCount ?? 0,
            bytesDownloaded: cur?.bytesDownloaded ?? 0,
            bytesTotal: cur?.bytesTotal ?? remoteSize?.totalBytes ?? 0,
            phase: 'failed',
            error: message,
          },
        };
      });
      setSherpaBusy(null);
    }
  };

  const handleCancelSherpaDownload = async () => {
    if (sherpaBusy !== 'download') return;
    setSherpaDownloadCancelRequested(true);
    try {
      await cancelSherpaOnnxAsrDownload(selectedSherpaAlias);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setSherpaDownloadCancelRequested(false);
    }
  };

  const handleDownload = async (modelId: string) => {
    setBusyModelId(modelId);
    // On re-download, seed with a locally known value before the first backend event arrives so the bar
    // doesn't jump from 0% to the real position. Priority: last progress entry (usually gone after cancel)
    // → downloadedBytes in models (optimistically written on cancel).
    const model = models.find((m) => m.id === modelId);
    const initialDownloaded = progress[modelId]?.bytesDownloaded ?? model?.downloadedBytes ?? 0;
    setProgress((prev) => ({
      ...prev,
      [modelId]: {
        modelId,
        file: '',
        fileIndex: 0,
        fileCount: remoteSizes[modelId]?.fileCount ?? 0,
        bytesDownloaded: initialDownloaded,
        bytesTotal: remoteSizes[modelId]?.totalBytes ?? 0,
        phase: 'started',
        error: null,
      },
    }));
    try {
      await downloadLocalAsrModel(
        modelId,
        effectiveModelMirror(modelId, settings?.mirror ?? 'huggingface'),
      );
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setProgress((prev) => {
        const cur = prev[modelId];
        if (cur?.phase === 'started') {
          return {
            ...prev,
            [modelId]: {
              ...cur,
              phase: 'failed',
              error: e instanceof Error ? e.message : String(e),
            },
          };
        }
        return prev;
      });
    } finally {
      setBusyModelId(null);
    }
  };

  const handleCancel = async (modelId: string) => {
    // bytesDownloaded in the Progress event is backend in_flight + already_done — the real byte count
    const lastBytes = progress[modelId]?.bytesDownloaded ?? 0;
    try {
      await cancelLocalAsrDownload(modelId);
      setProgress((prev) => {
        const next = { ...prev };
        delete next[modelId];
        return next;
      });
      // Optimistic update: flip hasPartial immediately instead of waiting for the listener's 200ms refresh
      if (lastBytes > 0) {
        setModels((prev) =>
          prev.map((m) => (m.id === modelId ? { ...m, downloadedBytes: lastBytes } : m)),
        );
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleDelete = async (modelId: string) => {
    if (
      !window.confirm(
        t('localAsr.deleteConfirm', {
          name: modelId,
        }),
      )
    ) {
      return;
    }
    setBusyModelId(modelId);
    try {
      await deleteLocalAsrModel(modelId);
      setProgress((prev) => {
        const next = { ...prev };
        delete next[modelId];
        return next;
      });
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyModelId(null);
    }
  };

  const handleRevealModelDir = async (modelId: string) => {
    setBusyModelId(modelId);
    try {
      setError(null);
      await revealLocalAsrModelDir(modelId);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyModelId(null);
    }
  };

  // Clean up the staging directory of an interrupted download; installed models are unaffected (guaranteed by the backend).
  const handleCleanupIncomplete = async (modelId: string) => {
    setBusyModelId(modelId);
    try {
      setError(null);
      await cleanupIncompleteLocalAsrModel(modelId);
      emitSaved('saved', t('common.saved'));
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyModelId(null);
    }
  };

  const handleKeepLoadedChange = async (seconds: number) => {
    try {
      await setLocalAsrKeepLoadedSecs(seconds);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleReleaseEngine = async () => {
    try {
      await releaseLocalAsrEngine();
      await refreshEngineStatus();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handlePreload = async () => {
    try {
      // After loading, the backend emits `local-asr:engine-changed`; the frontend updates without polling.
      await preloadLocalAsr();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  // Set as the current model first (including switching the active provider to the local engine), then run
  // the built-in audio test, so Qwen3 and Whisper can be compared for load/transcribe latency on one page.
  const handleTest = async (
    modelId: string,
    provider: 'local-qwen3-mlx' | 'local-qwen3-c' | 'local-whisper' = prefs?.activeAsrProvider ===
    'local-qwen3-c'
      ? 'local-qwen3-c'
      : supportsQwen3Mlx
        ? 'local-qwen3-mlx'
        : 'local-qwen3-c',
  ) => {
    try {
      await activateLocalAsr('generic', modelId, provider);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return;
    }
    setTestingModelId(modelId);
    setTestResults((prev) => {
      const next = { ...prev };
      delete next[modelId];
      return next;
    });
    try {
      const result = await testLocalAsrModel(modelId);
      setTestResults((prev) => ({ ...prev, [modelId]: result }));
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setTestResults((prev) => ({
        ...prev,
        [modelId]: { error: message },
      }));
    } finally {
      setTestingModelId(null);
    }
  };

  const handleMirrorChange = async (mirror: string) => {
    try {
      await setLocalAsrMirror(mirror);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const engineAvailable = settings?.engineAvailable ?? false;
  // Reliable "downloading" test: busyModelId is cleared right after the download starts (the Rust command
  // returns synchronously; the download runs on a backend thread), so the progress entry's phase is the
  // trustworthy signal. Used for: 1) dialog busy — a mask click must not close the dialog; 2) disabling
  // the "+ download new model" button while a download is in flight.
  const anyDownloadInFlight =
    Object.values(progress).some((p) => p.phase === 'started' || p.phase === 'progress') ||
    Object.values(sherpaDownloadProgress).some(
      (p) => p.phase === 'started' || p.phase === 'progress',
    );
  const foundryPlatformAvailable = isWindowsLikePlatform();
  const foundryAvailable =
    foundryStatus?.available === true ||
    (foundryPlatformAvailable && foundryStatus?.available !== false);
  const foundryDefault = prefs?.activeAsrProvider === 'foundry-local-whisper';
  const selectedFoundryModel =
    FOUNDRY_LOCAL_ASR_MODELS.find((model) => model.alias === selectedFoundryAlias) ??
    FOUNDRY_LOCAL_ASR_MODELS[0];
  const selectedFoundryCatalog = foundryCatalog.find(
    (model) => model.alias === selectedFoundryAlias,
  );
  const selectedFoundryDisplayName =
    selectedFoundryCatalog?.displayName ?? t(selectedFoundryModel.labelKey);
  const selectedFoundrySizeMb = formatFoundrySizeMb(selectedFoundryCatalog?.fileSizeMb);
  const selectedFoundrySizeLabel = selectedFoundrySizeMb
    ? t('localAsr.foundryApproxSizeMb', { mb: selectedFoundrySizeMb })
    : t('localAsr.sizeUnknown');
  const selectedFoundryDownloadLabel = selectedFoundryCatalog?.cached
    ? t('localAsr.downloadedBadge')
    : t('localAsr.notDownloadedBadge');
  const selectedFoundryLanguageHint = normalizeFoundryLanguageHintForUi(
    prefs?.foundryLocalAsrLanguageHint ?? '',
  );
  const selectedFoundryRuntimeSource = normalizeFoundryRuntimeSourceForUi(
    prefs?.foundryLocalRuntimeSource ?? foundryStatus?.runtimeSource ?? 'auto',
  );
  const foundryPrepareLabel =
    foundryBusy === 'prepare'
      ? foundryCancelRequested
        ? t('localAsr.foundryCancelling')
        : t('localAsr.foundryPreparing')
      : foundryProgress?.phase === 'failed'
        ? t('localAsr.foundryRetryPrepare')
        : t('localAsr.foundryPrepare');
  const sherpaAvailable =
    sherpaStatus?.available === true ||
    (foundryPlatformAvailable && sherpaStatus?.available !== false);
  const sherpaDefault = prefs?.activeAsrProvider === 'sherpa-onnx-local';
  const selectedSherpaModel =
    SHERPA_ONNX_ASR_MODELS.find((model) => model.alias === selectedSherpaAlias) ??
    SHERPA_ONNX_ASR_MODELS[0];
  const selectedSherpaUsesReleaseArchive = selectedSherpaAlias === 'qwen3-asr-0.6b-int8';
  const selectedSherpaMirrorValue = selectedSherpaUsesReleaseArchive
    ? 'github-release'
    : effectiveSherpaMirror(settings?.mirror ?? 'huggingface');
  const selectedSherpaCatalog = sherpaCatalog.find((model) => model.alias === selectedSherpaAlias);
  const selectedSherpaDisplayName =
    selectedSherpaCatalog?.displayName ?? t(selectedSherpaModel.labelKey);
  const selectedSherpaRemoteSize = sherpaRemoteSizes[selectedSherpaAlias];
  const selectedSherpaDownloadProgress = sherpaDownloadProgress[selectedSherpaAlias];
  const selectedSherpaDownloadedBytes = selectedSherpaCatalog?.downloadedBytes ?? 0;
  const selectedSherpaProgressBytes = selectedSherpaDownloadProgress?.bytesDownloaded ?? 0;
  const selectedSherpaPartialBytes = Math.max(
    selectedSherpaProgressBytes,
    selectedSherpaDownloadedBytes,
  );
  const isSherpaDownloading =
    selectedSherpaDownloadProgress?.phase === 'started' ||
    selectedSherpaDownloadProgress?.phase === 'progress';
  const hasSherpaPartial =
    selectedSherpaCatalog?.cached !== true &&
    selectedSherpaDownloadProgress?.phase !== 'finished' &&
    selectedSherpaPartialBytes > 0;
  const selectedSherpaHasLocalFiles =
    selectedSherpaCatalog?.cached === true || selectedSherpaDownloadedBytes > 0;
  const canDeleteSelectedSherpa = selectedSherpaHasLocalFiles || hasSherpaPartial;
  const showSherpaDownloadProgress =
    isSherpaDownloading || selectedSherpaDownloadProgress?.phase === 'failed' || hasSherpaPartial;
  const selectedSherpaDownloadProgressForDisplay =
    selectedSherpaDownloadProgress ??
    (hasSherpaPartial
      ? {
          modelId: selectedSherpaAlias,
          file: '',
          fileIndex: 0,
          fileCount: selectedSherpaRemoteSize?.fileCount ?? 0,
          bytesDownloaded: selectedSherpaDownloadedBytes,
          bytesTotal: selectedSherpaRemoteSize?.totalBytes ?? 0,
          phase: 'progress' as const,
          error: null,
        }
      : undefined);
  const selectedSherpaSizeMb = formatFoundrySizeMb(selectedSherpaCatalog?.fileSizeMb);
  const selectedSherpaSizeLabel = selectedSherpaRemoteSize?.loading
    ? t('localAsr.sizeLoading')
    : selectedSherpaRemoteSize?.totalBytes
      ? `${formatBytes(selectedSherpaRemoteSize.totalBytes)} · ${selectedSherpaRemoteSize.fileCount} ${t('localAsr.files')}`
      : selectedSherpaSizeMb
        ? t('localAsr.foundryApproxSizeMb', { mb: selectedSherpaSizeMb })
        : t('localAsr.sizeUnknown');
  const selectedSherpaDownloadLabel = selectedSherpaCatalog?.cached
    ? t('localAsr.downloadedBadge')
    : t('localAsr.notDownloadedBadge');
  const selectedSherpaLanguageHint = normalizeSherpaLanguageHintForUi(
    prefs?.sherpaOnnxLanguageHint ?? '',
  );
  const sherpaModelOptions = useMemo(
    () =>
      SHERPA_ONNX_ASR_MODELS.map((model) => {
        const catalog = sherpaCatalog.find((item) => item.alias === model.alias);
        const remoteSize = sherpaRemoteSizes[model.alias];
        const sizeMb = formatFoundrySizeMb(catalog?.fileSizeMb);
        const sizeLabel = remoteSize?.totalBytes
          ? formatBytes(remoteSize.totalBytes)
          : sizeMb
            ? t('localAsr.foundryApproxSizeMb', { mb: sizeMb })
            : '';
        return {
          value: model.alias,
          label: `${t(model.labelKey)}${sizeLabel ? ` · ${sizeLabel}` : ''}`,
        };
      }),
    [sherpaCatalog, sherpaRemoteSizes, t],
  );
  const sherpaLanguageOptions = useMemo(
    () => [
      {
        value: '',
        label: t('localAsr.foundryLanguageAuto'),
      },
      {
        value: 'zh',
        label: t('localAsr.foundryLanguageZh'),
      },
      {
        value: 'en',
        label: t('localAsr.foundryLanguageEn'),
      },
      {
        value: 'ja',
        label: t('localAsr.sherpaLanguageJa'),
      },
      {
        value: 'ko',
        label: t('localAsr.sherpaLanguageKo'),
      },
      {
        value: 'yue',
        label: t('localAsr.sherpaLanguageYue'),
      },
    ],
    [t],
  );
  const sherpaPrepareLabel =
    sherpaBusy === 'prepare'
      ? sherpaCancelRequested
        ? t('localAsr.foundryCancelling')
        : t('localAsr.sherpaPreparing')
      : sherpaProgress?.phase === 'failed'
        ? t('localAsr.foundryRetryPrepare')
        : t('localAsr.sherpaPrepare');

  // Unified model entries for the two-pane board (Qwen3 / sherpa-onnx / foundry normalized).
  // allSidebarEntries = full catalog (used by the download dialog; lists not-downloaded/downloading/downloaded
  // so the dialog can pick every fetchable model).
  // sidebarEntries = only downloaded / downloading models (board; not-downloaded ones come from the
  // "download new model" dialog).
  const allSidebarEntries = useMemo<SidebarModelEntry[]>(() => {
    const entries: SidebarModelEntry[] = [];
    // macOS: Qwen3 / Whisper engines
    for (const m of models) {
      if (m.runtime !== 'generic' || !isLocalAsrModelSupportedOnOs(m, OS)) continue;
      const isWhisper = m.family === 'whisper';
      const isDownloading =
        Boolean(progress[m.id]) &&
        (progress[m.id]?.phase === 'started' || progress[m.id]?.phase === 'progress');
      entries.push({
        id: m.id,
        name: m.id.replace(/^qwen3-asr-/, 'Qwen3-ASR ').replace(/^whisper-/, 'Whisper '),
        // Catalog metadata comes from the Core descriptor snapshot; display no longer depends on live HuggingFace.
        displayName: m.displayName || undefined,
        languages: m.languages?.length ? m.languages : undefined,
        sizeBytes: m.sizeBytes ?? undefined,
        partialBytes:
          !m.isDownloaded && !isDownloading && m.downloadedBytes > 0
            ? m.downloadedBytes
            : undefined,
        repo: m.hfRepo,
        remoteBytes:
          remoteSizes[m.id]?.totalBytes || (m.isDownloaded ? m.downloadedBytes : undefined),
        isDownloaded: m.isDownloaded,
        isDownloading,
        percent: isDownloading
          ? progress[m.id] && progress[m.id]?.bytesTotal > 0
            ? (progress[m.id]!.bytesDownloaded / progress[m.id]!.bytesTotal) * 100
            : null
          : null,
        isActive:
          settings?.activeModel === m.id &&
          (isWhisper
            ? prefs?.activeAsrProvider === 'local-whisper'
            : ['local-qwen3', 'local-qwen3-mlx', 'local-qwen3-c'].includes(
                prefs?.activeAsrProvider ?? '',
              )),
        engine: isWhisper ? 'whisper' : 'qwen3',
        runtimeLabel: isWhisper
          ? 'macOS · whisper.cpp'
          : OS === 'mac'
            ? supportsQwen3Mlx
              ? 'macOS · MLX (Metal) / C (CPU)'
              : 'macOS · C (CPU)'
            : 'Linux · C (CPU)',
        downloadError:
          progress[m.id]?.phase === 'failed' ? progress[m.id]?.error || t('localAsr.failed') : null,
      });
    }
    // Windows：sherpa-onnx + foundry
    for (const c of IS_WINDOWS ? sherpaCatalog : []) {
      const isDownloading =
        Boolean(sherpaDownloadProgress[c.alias]) &&
        (sherpaDownloadProgress[c.alias]?.phase === 'started' ||
          sherpaDownloadProgress[c.alias]?.phase === 'progress');
      entries.push({
        id: c.alias,
        name: c.displayName || c.alias,
        displayName: c.displayName || undefined,
        sizeBytes: c.fileSizeMb != null ? c.fileSizeMb * 1024 * 1024 : undefined,
        remoteBytes:
          sherpaRemoteSizes[c.alias]?.totalBytes ||
          (c.fileSizeMb != null ? c.fileSizeMb * 1024 * 1024 : undefined),
        isDownloaded: c.cached,
        isDownloading,
        percent: isDownloading
          ? sherpaDownloadProgress[c.alias] && sherpaDownloadProgress[c.alias]?.bytesTotal > 0
            ? (sherpaDownloadProgress[c.alias]!.bytesDownloaded /
                sherpaDownloadProgress[c.alias]!.bytesTotal) *
              100
            : null
          : null,
        isActive:
          sherpaStatus?.activeModel === c.alias && prefs?.activeAsrProvider === 'sherpa-onnx-local',
        engine: 'sherpa',
        runtimeLabel: 'Windows · sherpa-onnx',
        downloadError:
          sherpaDownloadProgress[c.alias]?.phase === 'failed'
            ? sherpaDownloadProgress[c.alias]?.error || t('localAsr.failed')
            : null,
      });
    }
    for (const c of IS_WINDOWS ? foundryCatalog : []) {
      // Foundry downloads happen inside prepare (runtime/model/load phases) while cached is still false;
      // the prepare progress is what marks "downloading" and keeps the entry alive.
      const isDownloading =
        foundryProgress?.modelAlias === c.alias &&
        (foundryProgress.phase === 'runtime' ||
          foundryProgress.phase === 'model' ||
          foundryProgress.phase === 'load');
      entries.push({
        id: c.alias,
        name: c.displayName || c.alias,
        displayName: c.displayName || undefined,
        sizeBytes: c.fileSizeMb != null ? c.fileSizeMb * 1024 * 1024 : undefined,
        remoteBytes: c.fileSizeMb != null ? c.fileSizeMb * 1024 * 1024 : undefined,
        isDownloaded: c.cached,
        isDownloading,
        percent: isDownloading && foundryProgress?.percent != null ? foundryProgress.percent : null,
        isActive:
          foundryStatus?.activeModel === c.alias &&
          prefs?.activeAsrProvider === 'foundry-local-whisper',
        engine: 'foundry',
        runtimeLabel: 'Windows · Foundry Local',
      });
    }
    return entries;
  }, [
    models,
    supportsQwen3Mlx,
    remoteSizes,
    progress,
    settings?.activeModel,
    prefs?.activeAsrProvider,
    sherpaCatalog,
    sherpaRemoteSizes,
    sherpaDownloadProgress,
    sherpaStatus?.activeModel,
    foundryCatalog,
    foundryProgress,
    foundryStatus?.activeModel,
    t,
  ]);

  // The board shows only downloaded / downloading models (downloading ones must show live progress).
  const sidebarEntries = useMemo<SidebarModelEntry[]>(
    () => allSidebarEntries.filter((e) => e.isDownloaded || e.isDownloading),
    [allSidebarEntries],
  );

  // When the dialog opens and the board selection is missing from the full catalog (fresh installs with
  // nothing selected), write the dialog's default highlight back into selectedModelId — dialog highlight
  // and board state stay one value, so switching / starting a download never diverges.
  useEffect(() => {
    if (!downloadDialogOpen) return;
    const valid = allSidebarEntries.some((e) => e.id === selectedModelId);
    if (valid) return;
    const fallback = allSidebarEntries.find((e) => !e.isDownloaded) ?? allSidebarEntries[0] ?? null;
    const nextSelectedId = fallback?.id ?? null;
    if (nextSelectedId !== selectedModelId) setSelectedModelId(nextSelectedId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [downloadDialogOpen, allSidebarEntries, selectedModelId]);

  const selectedEntry = sidebarEntries.find((e) => e.id === selectedModelId) ?? null;

  // Sidebar selection default: after first render with no selection, pick the first downloaded model.
  useLayoutEffect(() => {
    // While the download dialog is open, highlighting a not-downloaded model inside it is valid (selecting
    // prepares a download); the board's fallback must not steal the dialog highlight. Fallback resumes after close.
    if (downloadDialogOpen) return;
    // When the selection was deleted (or never made), fall back to the first downloaded model so the
    // sidebar keeps a highlight and the detail panel doesn't sit in an empty state.
    const stillExists =
      selectedModelId !== null && sidebarEntries.some((e) => e.id === selectedModelId);
    if (stillExists) return;
    const firstDownloaded = sidebarEntries.find((e) => e.isDownloaded);
    const nextSelectedId = firstDownloaded?.id ?? sidebarEntries[0]?.id ?? null;
    // Empty catalogs and metadata refreshes can leave the same selection.
    // Do not schedule a layout-phase update when nothing has changed.
    if (nextSelectedId !== selectedModelId) setSelectedModelId(nextSelectedId);
  }, [sidebarEntries, selectedModelId, downloadDialogOpen]);

  // Dispatch engine actions from the sidebar/board. There is no separate setActive — activation means
  // picking the local provider in ASR dictation; "load and test" sets the model as the one in use.
  const dispatchEntryAction = (
    entry: SidebarModelEntry,
    action: 'download' | 'delete' | 'reveal',
  ) => {
    if (entry.engine === 'qwen3') {
      if (action === 'download') void handleDownload(entry.id);
      else if (action === 'delete') void handleDelete(entry.id);
      else if (action === 'reveal') void handleRevealModelDir(entry.id);
    } else if (entry.engine === 'whisper') {
      if (action === 'download') void handleDownload(entry.id);
      else if (action === 'delete') void handleDelete(entry.id);
      else if (action === 'reveal') void handleRevealModelDir(entry.id);
    } else if (entry.engine === 'sherpa') {
      const alias = entry.id as SherpaOnnxModelAlias;
      if (action === 'download') {
        setSelectedSherpaAlias(alias);
        // Pass the alias explicitly: the setTimeout closure can't see new state (the handler reads this
        // render's selectedSherpaAlias); omitting it would operate on the previous model.
        window.setTimeout(() => void handleDownloadSherpa(alias), 0);
      } else if (action === 'delete') {
        setSelectedSherpaAlias(alias);
        window.setTimeout(() => void handleDeleteSherpa(alias), 0);
      }
    } else if (entry.engine === 'foundry') {
      const alias = entry.id as FoundryLocalAsrModelAlias;
      if (action === 'download') {
        setSelectedFoundryAlias(alias);
        void handleEnableAndPrepareFoundry(alias);
      } else if (action === 'delete') {
        setSelectedFoundryAlias(alias);
        void handleDeleteFoundry(alias);
      }
    }
  };

  // Download dialog "start download": dispatch the dialog's current selection to the matching engine's
  // download entry. The dialog lists the full catalog (allSidebarEntries), so the selection may not be in
  // the board's filtered list; when the dialog defaulted to its first item, selectedModelId may still be
  // null — fall back to the first not-downloaded entry.
  const startDownloadFromDialog = () => {
    const dialogEntry =
      allSidebarEntries.find((e) => e.id === selectedModelId) ??
      allSidebarEntries.find((e) => !e.isDownloaded) ??
      null;
    if (!dialogEntry || dialogEntry.isDownloaded) return;
    dispatchEntryAction(dialogEntry, 'download');
    setDownloadDialog(false);
  };

  const selectedEntryRemote = selectedEntry
    ? selectedEntry.engine === 'qwen3'
      ? remoteSizes[selectedEntry.id]
      : selectedEntry.engine === 'whisper' || selectedEntry.engine === 'sherpa'
        ? selectedEntry.engine === 'whisper'
          ? remoteSizes[selectedEntry.id]
          : sherpaRemoteSizes[selectedEntry.id]
        : null
    : null;
  const selectedEntryProgress =
    selectedEntry?.engine === 'qwen3' || selectedEntry?.engine === 'whisper'
      ? progress[selectedEntry.id]
      : selectedEntry?.engine === 'sherpa'
        ? sherpaDownloadProgress[selectedEntry.id]
        : undefined;
  const reloadModels = async () => {
    setCatalogRefreshing(true);
    try {
      await refresh();
    } finally {
      setCatalogRefreshing(false);
    }
  };
  useEffect(() => {
    if (downloadDialogOpen || !catalogReloadRequestedRef.current) return;
    // Run after the dialog poller's cleanup advances the refresh generation.
    catalogReloadRequestedRef.current = false;
    void reloadModels();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [downloadDialogOpen]);
  const modelLoadPending = (settings === null && !error) || catalogRefreshing;
  const downloadDisabled =
    modelLoadPending || busyModelId !== null || sherpaBusy !== null || anyDownloadInFlight;
  const visibleError =
    error || allSidebarEntries.find((entry) => entry.downloadError)?.downloadError;

  return (
    <LocalAsrContentWrapper embedded={embedded}>
      {!embedded && (
        <PageHeader
          kicker={t('localAsr.kicker')}
          title={t('localAsr.title')}
          desc={t('localAsr.desc')}
        />
      )}

      {/* ─── The top-right download progress overlay is now global (mounted at the App root,
                 present on every page) and no longer rendered here; in-page progress still drives
                 the board's detail bar via progress / sherpaDownloadProgress. ─── */}

      {!embedded && (
        /* Performance/quality expectation warning — in embedded mode AdvancedSection renders it itself to avoid duplication. */
        <Card
          style={{
            marginBottom: 16,
            background: 'var(--ol-surface-2)',
          }}
        >
          <div
            style={{
              fontSize: 13,
              color: 'var(--ol-ink-2)',
              lineHeight: 1.6,
            }}
          >
            ⚠️ {t('localAsr.performanceWarning')}
          </div>
        </Card>
      )}

      {/* Windows manages models through the Foundry / sherpa cards below;
                 macOS / Linux still need this board for Qwen3 / Whisper models. */}
      {visibleError && (
        <div className="ol-model-error" role="alert" style={{ marginBottom: 20 }}>
          {visibleError}
        </div>
      )}
      {!IS_WINDOWS && (
        <section className="ol-model-manager">
          {sidebarEntries.length > 0 ? (
            <>
              <div className="ol-model-manager-heading">
                <div>
                  <h3>{t('localAsr.modelSelectTitle')}</h3>
                  <p>{t('localAsr.modelSelectDesc')}</p>
                </div>
                <Btn
                  variant="blue"
                  disabled={downloadDisabled}
                  onClick={() => setDownloadDialog(true)}
                >
                  {t('localAsr.downloadNewModel')}
                </Btn>
              </div>
              <div className={`ol-model-workspace${stackLayout ? ' is-stacked' : ''}`}>
                <ModelSidebar
                  entries={sidebarEntries}
                  selectedId={selectedModelId}
                  onSelect={(id) => {
                    setSelectedModelId(id);
                    // Verify disk state the moment a model is selected (files may have been deleted
                    // externally) and reflect it in list and details immediately, without waiting for the 3s poll.
                    void refresh();
                  }}
                />
                <div className="ol-model-selected-panel">
                  <ModelDetailPanel
                    entry={selectedEntry}
                    fileCount={selectedEntryRemote?.fileCount ?? null}
                    mirrorLabel={
                      selectedEntry?.engine === 'qwen3' || selectedEntry?.engine === 'whisper'
                        ? effectiveModelMirror(selectedEntry.id, settings?.mirror ?? 'huggingface')
                        : selectedEntry?.engine === 'sherpa'
                          ? effectiveSherpaMirror(settings?.mirror ?? 'huggingface')
                          : undefined
                    }
                    progress={selectedEntryProgress}
                    busy={busyModelId !== null || sherpaBusy !== null || testingModelId !== null}
                    onDownload={() =>
                      selectedEntry && dispatchEntryAction(selectedEntry, 'download')
                    }
                    onCancel={() => {
                      if (!selectedEntry) return;
                      if (selectedEntry.engine === 'qwen3' || selectedEntry.engine === 'whisper')
                        void handleCancel(selectedEntry.id);
                      else if (selectedEntry.engine === 'sherpa') void handleCancelSherpaDownload();
                    }}
                    onDelete={() => selectedEntry && dispatchEntryAction(selectedEntry, 'delete')}
                    onReveal={() => selectedEntry && dispatchEntryAction(selectedEntry, 'reveal')}
                    onCleanup={
                      selectedEntry &&
                      (selectedEntry.engine === 'qwen3' || selectedEntry.engine === 'whisper') &&
                      selectedEntry.partialBytes
                        ? () => void handleCleanupIncomplete(selectedEntry.id)
                        : undefined
                    }
                    onTest={() => {
                      if (
                        selectedEntry?.engine === 'qwen3' ||
                        selectedEntry?.engine === 'whisper'
                      ) {
                        void handleTest(
                          selectedEntry.id,
                          selectedEntry.engine === 'whisper'
                            ? 'local-whisper'
                            : supportsQwen3Mlx
                              ? 'local-qwen3-mlx'
                              : 'local-qwen3-c',
                        );
                      }
                    }}
                    showTest={
                      selectedEntry?.engine === 'qwen3' || selectedEntry?.engine === 'whisper'
                    }
                    testResult={selectedEntry ? (testResults[selectedEntry.id] ?? null) : null}
                    testing={
                      (selectedEntry?.engine === 'qwen3' || selectedEntry?.engine === 'whisper') &&
                      testingModelId === selectedEntry.id
                    }
                  />
                </div>
              </div>
            </>
          ) : (
            <div className="ol-model-empty" aria-busy={modelLoadPending}>
              <span className="ol-model-empty-icon">
                <Icon name="download" size={25} />
              </span>
              <h3>{modelLoadPending ? t('common.loading') : t('localAsr.libraryEmptyTitle')}</h3>
              <p className="ol-model-muted">{t('localAsr.libraryEmptyDesc')}</p>
              <div className="ol-model-actions">
                <Btn
                  variant="blue"
                  disabled={downloadDisabled}
                  onClick={() => setDownloadDialog(true)}
                >
                  {t('localAsr.downloadNewModel')}
                </Btn>
                <Btn
                  variant="ghost"
                  disabled={catalogRefreshing}
                  onClick={() => void reloadModels()}
                >
                  {t('localAsr.reloadCatalog')}
                </Btn>
              </div>
            </div>
          )}
        </section>
      )}

      {/* ─── Collapsed: download and storage settings (mirror · model storage location · in-memory engine) —
                 collapsed by default, opened manually. Everyday download / manage / test flows don't need them. ─── */}
      <div className="ol-model-advanced">
        <Collapsible
          title={t('localAsr.downloadSettingsTitle')}
          desc={t('localAsr.downloadSettingsDesc')}
        >
          {IS_QWEN_PLATFORM && (
            <>
              <div className="ol-model-setting-group">
                <div
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    gap: 16,
                    flexWrap: 'wrap',
                  }}
                >
                  <div>
                    <div
                      style={{
                        fontSize: 12,
                        fontWeight: 600,
                        color: 'var(--ol-ink-4)',
                        marginBottom: 4,
                      }}
                    >
                      {t('localAsr.mirrorLabel')}
                    </div>
                    <div
                      style={{
                        fontSize: 13,
                        color: 'var(--ol-ink-3)',
                      }}
                    >
                      {t('localAsr.mirrorDesc')}
                    </div>
                  </div>

                  <SelectLite
                    value={settings?.mirror ?? 'huggingface'}
                    onChange={(v) => void handleMirrorChange(v)}
                    ariaLabel={t('localAsr.mirrorLabel')}
                    options={[
                      {
                        value: 'huggingface',
                        label: t('localAsr.mirrorHuggingface'),
                      },
                      {
                        value: 'hf-mirror',
                        label: t('localAsr.mirrorHfMirror'),
                      },
                      {
                        value: 'modelscope',
                        label: t('localAsr.mirrorModelscope'),
                      },
                    ]}
                    style={{ fontSize: 13, height: 31, minWidth: 200 }}
                  />
                </div>
              </div>
              {/* Runtime settings card: in-memory engine state + release timer + release now */}
              {engineAvailable && (
                <div className="ol-model-setting-group">
                  <div
                    style={{
                      display: 'flex',
                      flexDirection: 'column',
                      gap: 12,
                    }}
                  >
                    <div
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'space-between',
                        gap: 12,
                        flexWrap: 'wrap',
                      }}
                    >
                      <div>
                        <div
                          style={{
                            fontSize: 12,
                            fontWeight: 600,
                            color: 'var(--ol-ink-4)',
                            marginBottom: 4,
                          }}
                        >
                          {t('localAsr.engineStatusLabel')}
                        </div>
                        <div
                          style={{
                            fontSize: 13,
                            color: 'var(--ol-ink-3)',
                          }}
                        >
                          {engineStatus?.loaded
                            ? t('localAsr.engineLoaded', {
                                model: engineStatus.modelId ?? '',
                              })
                            : t('localAsr.engineUnloaded')}
                        </div>
                      </div>
                      <div style={{ display: 'flex', gap: 8 }}>
                        {engineStatus?.loaded ? (
                          <Btn variant="ghost" size="sm" onClick={() => void handleReleaseEngine()}>
                            {t('localAsr.releaseNow')}
                          </Btn>
                        ) : (
                          <Btn variant="ghost" size="sm" onClick={() => void handlePreload()}>
                            {t('localAsr.loadNow')}
                          </Btn>
                        )}
                      </div>
                    </div>
                    <div
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'space-between',
                        gap: 12,
                        flexWrap: 'wrap',
                      }}
                    >
                      <div style={{ minWidth: 0 }}>
                        <div
                          style={{
                            fontSize: 12,
                            fontWeight: 600,
                            color: 'var(--ol-ink-4)',
                            marginBottom: 4,
                          }}
                        >
                          {t('localAsr.keepLoadedLabel')}
                        </div>
                        <div
                          style={{
                            fontSize: 12,
                            color: 'var(--ol-ink-3)',
                            lineHeight: 1.5,
                          }}
                        >
                          {t('localAsr.keepLoadedDesc')}
                        </div>
                      </div>

                      <SelectLite
                        value={String(engineStatus?.keepLoadedSecs ?? 300)}
                        onChange={(v) => void handleKeepLoadedChange(Number(v))}
                        ariaLabel={t('localAsr.keepLoadedLabel')}
                        options={keepLoadedOptions}
                        style={{ fontSize: 13, height: 31, minWidth: 200 }}
                      />
                    </div>
                  </div>
                </div>
              )}
            </>
          )}
          <div className="ol-model-setting-group">
            <div
              style={{
                display: 'flex',
                flexDirection: 'column',
                gap: 12,
              }}
            >
              <div
                style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  gap: 16,
                  flexWrap: 'wrap',
                }}
              >
                <div style={{ minWidth: 0, flex: '1 1 360px' }}>
                  <div
                    style={{
                      fontSize: 14,
                      fontWeight: 700,
                      color: 'var(--ol-ink)',
                      marginBottom: 6,
                    }}
                  >
                    {t('localAsr.storageTitle')}
                  </div>
                  <div
                    style={{
                      fontSize: 12.5,
                      color: 'var(--ol-ink-3)',
                      lineHeight: 1.6,
                    }}
                  >
                    <div>
                      <span style={{ color: 'var(--ol-ink-4)' }}>
                        {t('localAsr.storageBaseDir')}:{' '}
                      </span>
                      <code>{settings?.modelsBaseDir ?? t('localAsr.storageDefault')}</code>
                    </div>
                    <div>
                      <span style={{ color: 'var(--ol-ink-4)' }}>
                        {t('localAsr.storageModelsRoot')}:{' '}
                      </span>
                      <code>{settings?.modelsRootDir ?? '—'}</code>
                    </div>
                  </div>
                </div>
                <div
                  style={{
                    display: 'flex',
                    gap: 8,
                    flexWrap: 'wrap',
                    justifyContent: 'flex-end',
                    alignContent: 'flex-start',
                  }}
                >
                  <Btn
                    variant="primary"
                    size="sm"
                    disabled={storageBusy}
                    onClick={() => void handleChooseModelsBaseDir()}
                  >
                    {storageBusy ? t('common.loading') : t('localAsr.storageChoose')}
                  </Btn>
                  <Btn
                    variant="ghost"
                    size="sm"
                    disabled={storageBusy || !settings?.modelsBaseDir}
                    onClick={() => void handleResetModelsBaseDir()}
                  >
                    {t('localAsr.storageReset')}
                  </Btn>
                  <Btn
                    variant="ghost"
                    size="sm"
                    disabled={storageBusy}
                    onClick={() => void handleRevealModelsRoot()}
                  >
                    {t('localAsr.storageReveal')}
                  </Btn>
                </div>
              </div>
              <div
                style={{
                  fontSize: 12,
                  color: 'var(--ol-ink-4)',
                  lineHeight: 1.55,
                }}
              >
                {t('localAsr.storageDesc')}
              </div>
            </div>
          </div>
        </Collapsible>
      </div>
      {/* ─── Download dialog: model selection on the left, details on the right, start download at the bottom. ─── */}
      {downloadDialogOpen && (
        <DownloadDialog
          entries={allSidebarEntries}
          selectedId={selectedModelId}
          onSelect={setSelectedModelId}
          sizeOf={(id) => {
            const entry = allSidebarEntries.find((e) => e.id === id);
            return entry?.remoteBytes ?? null;
          }}
          fileCountOf={(id) => {
            const entry = allSidebarEntries.find((e) => e.id === id);
            if (!entry) return null;
            const remote =
              entry.engine === 'qwen3' || entry.engine === 'whisper'
                ? remoteSizes[id]
                : entry.engine === 'sherpa'
                  ? sherpaRemoteSizes[id]
                  : null;
            return remote?.fileCount ?? null;
          }}
          busy={busyModelId !== null || anyDownloadInFlight}
          loading={modelLoadPending}
          error={error}
          onRetryCatalog={() => {
            remoteInfoCache.clear();
            sherpaInfoCache.clear();
            modelCardCache.clear();
            // Keep the dialog refresh-generation guard: close first, then query.
            catalogReloadRequestedRef.current = true;
            setDownloadDialog(false);
          }}
          onRetryCard={(id) =>
            void ensureHfCard(id, effectiveModelMirror(id, settings?.mirror ?? 'huggingface'))
          }
          hfCardOf={(id) => {
            const state = hfCards[id];
            if (!state) return null;
            if ('loading' in state) {
              return state.loading
                ? { status: 'loading' as const }
                : {
                    status: 'error' as const,
                    message: state.error ?? '',
                  };
            }
            return { status: 'ok' as const, card: state };
          }}
          onStart={startDownloadFromDialog}
          onClose={() => setDownloadDialog(false)}
        />
      )}

      {/* ─── Group: download and manage (per-engine model fetch/prepare/download) ─── */}
      {IS_WINDOWS && <LocalAsrGroupTitle>{t('localAsr.groupDownload')}</LocalAsrGroupTitle>}

      {IS_WINDOWS && (
        <Card style={{ marginBottom: 16 }}>
          <div
            style={{
              display: 'flex',
              flexDirection: 'column',
              gap: 14,
            }}
          >
            <div
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                gap: 16,
                flexWrap: 'wrap',
              }}
            >
              <div style={{ minWidth: 0, flex: '1 1 360px' }}>
                <div
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 8,
                    marginBottom: 6,
                    flexWrap: 'wrap',
                  }}
                >
                  <div
                    style={{
                      fontSize: 14,
                      fontWeight: 700,
                      color: 'var(--ol-ink)',
                    }}
                  >
                    {t('localAsr.foundryTitle')}
                  </div>
                  {foundryDefault && (
                    <Pill tone="blue" size="sm">
                      {t('localAsr.activeBadge')}
                    </Pill>
                  )}
                  <Pill tone={foundryStatus?.available ? 'ok' : 'outline'} size="sm">
                    {foundryStatus?.available
                      ? t('localAsr.foundryAvailable')
                      : t('localAsr.foundryUnavailable')}
                  </Pill>
                  <Pill tone={foundryStatus?.runtimeReady ? 'ok' : 'outline'} size="sm">
                    {foundryStatus?.runtimeReady
                      ? t('localAsr.foundryRuntimeReady')
                      : t('localAsr.foundryRuntimeMissing')}
                  </Pill>
                </div>
                <div
                  style={{
                    fontSize: 13,
                    color: 'var(--ol-ink-3)',
                    lineHeight: 1.55,
                  }}
                >
                  {t('localAsr.foundryDesc')}
                </div>
              </div>
              <div
                ref={foundryControlsRef}
                style={{
                  display: 'flex',
                  gap: 10,
                  flexWrap: 'wrap',
                  justifyContent: 'flex-end',
                }}
              >
                <label
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 4,
                    fontSize: 11,
                    color: 'var(--ol-ink-4)',
                  }}
                >
                  {t('localAsr.foundrySelectedModel')}
                  {/* Use the official SelectLite; scroll preservation walks up
                                        from the row container ref to the scroll ancestor. */}
                  <SelectLite
                    value={selectedFoundryAlias}
                    onChange={(v) => {
                      const restoreScroll = preserveEmbeddedScroll(foundryControlsRef.current);
                      const nextAlias = v as FoundryLocalAsrModelAlias;
                      foundrySelectionDirty.current = true;
                      setCurrentFoundryAlias(nextAlias);
                      void refreshFoundryModelDir(nextAlias);
                      restoreScroll();
                    }}
                    disabled={foundryBusy !== null}
                    ariaLabel={t('localAsr.foundrySelectedModel')}
                    options={FOUNDRY_LOCAL_ASR_MODELS.map((model) => {
                      const catalog = foundryCatalog.find((item) => item.alias === model.alias);
                      const sizeMb = formatFoundrySizeMb(catalog?.fileSizeMb);
                      return {
                        value: model.alias,
                        label: `${t(model.labelKey)}${
                          sizeMb ? ` · ${t('localAsr.foundryApproxSizeMb', { mb: sizeMb })}` : ''
                        }`,
                      };
                    })}
                    style={{
                      fontSize: 13,
                      height: 31,
                      minWidth: 260,
                    }}
                  />
                </label>
                <label
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 4,
                    fontSize: 11,
                    color: 'var(--ol-ink-4)',
                  }}
                >
                  {t('localAsr.foundryRuntimeSourceLabel')}

                  <SelectLite
                    value={selectedFoundryRuntimeSource}
                    onChange={(v) => {
                      const restoreScroll = preserveEmbeddedScroll(foundryControlsRef.current);
                      void handleFoundryRuntimeSourceChange(
                        v as FoundryRuntimeSource,
                        restoreScroll,
                      );
                    }}
                    disabled={foundryBusy !== null}
                    ariaLabel={t('localAsr.foundryRuntimeSourceLabel')}
                    options={[
                      {
                        value: 'auto',
                        label: t('localAsr.foundryRuntimeSourceAuto'),
                      },
                      {
                        value: 'nuget',
                        label: t('localAsr.foundryRuntimeSourceNuget'),
                      },
                      {
                        value: 'ort-nightly',
                        label: t('localAsr.foundryRuntimeSourceOrtNightly'),
                      },
                    ]}
                    style={{
                      fontSize: 13,
                      height: 31,
                      minWidth: 200,
                    }}
                  />
                </label>
                <label
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 4,
                    fontSize: 11,
                    color: 'var(--ol-ink-4)',
                  }}
                >
                  {t('localAsr.foundryLanguageLabel')}

                  <SelectLite
                    value={selectedFoundryLanguageHint}
                    onChange={(v) => {
                      const restoreScroll = preserveEmbeddedScroll(foundryControlsRef.current);
                      void handleFoundryLanguageChange(
                        v as FoundryLocalAsrLanguageHint,
                        restoreScroll,
                      );
                    }}
                    disabled={foundryBusy !== null}
                    ariaLabel={t('localAsr.foundryLanguageLabel')}
                    options={[
                      {
                        value: '',
                        label: t('localAsr.foundryLanguageAuto'),
                      },
                      {
                        value: 'zh',
                        label: t('localAsr.foundryLanguageZh'),
                      },
                      {
                        value: 'en',
                        label: t('localAsr.foundryLanguageEn'),
                      },
                    ]}
                    style={{
                      fontSize: 13,
                      height: 31,
                      minWidth: 132,
                    }}
                  />
                </label>
                <label
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 4,
                    fontSize: 11,
                    color: 'var(--ol-ink-4)',
                  }}
                >
                  {t('localAsr.keepLoadedLabel')}

                  <SelectLite
                    value={String(foundryStatus?.keepLoadedSecs ?? 300)}
                    onChange={(v) => {
                      const restoreScroll = preserveEmbeddedScroll(foundryControlsRef.current);
                      void handleFoundryKeepLoadedChange(Number(v), restoreScroll);
                    }}
                    disabled={foundryBusy !== null}
                    ariaLabel={t('localAsr.keepLoadedLabel')}
                    options={keepLoadedOptions}
                    style={{
                      fontSize: 13,
                      height: 31,
                      minWidth: 200,
                    }}
                  />
                </label>
              </div>
            </div>

            <div
              style={{
                fontSize: 12.5,
                color: 'var(--ol-ink-3)',
                lineHeight: 1.6,
              }}
            >
              <div>
                <span style={{ color: 'var(--ol-ink-4)' }}>
                  {t('localAsr.foundrySelectedModel')}:{' '}
                </span>
                <strong>{selectedFoundryDisplayName}</strong>
                <span>
                  {' '}
                  · {selectedFoundrySizeLabel} · {selectedFoundryDownloadLabel}
                </span>
                <span> · {t(selectedFoundryModel.descKey)}</span>
              </div>
              <div>
                <span style={{ color: 'var(--ol-ink-4)' }}>
                  {t('localAsr.foundryRuntimeSourceLabel')}:{' '}
                </span>
                {t(
                  `localAsr.foundryRuntimeSource${selectedFoundryRuntimeSource === 'ort-nightly' ? 'OrtNightly' : selectedFoundryRuntimeSource === 'nuget' ? 'Nuget' : 'Auto'}`,
                )}
                <span> · {t('localAsr.foundryRuntimeSourceDesc')}</span>
              </div>
              <div>
                <span style={{ color: 'var(--ol-ink-4)' }}>
                  {t('localAsr.foundryLanguageLabel')}:{' '}
                </span>
                {selectedFoundryLanguageHint
                  ? t(
                      `localAsr.foundryLanguage${selectedFoundryLanguageHint === 'zh' ? 'Zh' : 'En'}`,
                    )
                  : t('localAsr.foundryLanguageAuto')}
                <span> · {t('localAsr.foundryLanguageDesc')}</span>
              </div>
              <div>
                <span style={{ color: 'var(--ol-ink-4)' }}>
                  {t('localAsr.foundryActiveModel')}:{' '}
                </span>
                {foundryStatus?.activeModel ?? 'whisper-small'}
              </div>
              <div>
                <span style={{ color: 'var(--ol-ink-4)' }}>{t('localAsr.modelDir')}: </span>
                <code>
                  {foundryModelDir?.alias === selectedFoundryAlias ? foundryModelDir.dir : '—'}
                </code>
              </div>
              <div>
                <span style={{ color: 'var(--ol-ink-4)' }}>
                  {t('localAsr.foundryLoadedModel')}:{' '}
                </span>
                {foundryStatus?.loadedModelId ?? t('localAsr.foundryNotLoaded')}
              </div>
              <div>{t('localAsr.keepLoadedDesc')}</div>
              {foundryStatus?.error && (
                <div style={{ color: '#9b2c2c' }}>
                  <span>{t('localAsr.foundryError')}: </span>
                  {foundryStatus.error}
                </div>
              )}
            </div>

            {(foundryBusy === 'prepare' || foundryProgress) && (
              <FoundryPrepareProgressBlock
                progress={foundryProgress}
                modelCached={selectedFoundryCatalog?.cached === true}
                cancelRequested={foundryCancelRequested}
              />
            )}

            <div
              style={{
                display: 'flex',
                gap: 8,
                flexWrap: 'wrap',
              }}
            >
              <Btn
                variant="blue"
                size="sm"
                disabled={foundryBusy !== null || !foundryAvailable}
                onClick={() => void handleEnableFoundry()}
              >
                {foundryBusy === 'enable'
                  ? t('localAsr.foundryEnabling')
                  : t('localAsr.foundrySetDefault')}
              </Btn>
              <Btn
                variant="primary"
                size="sm"
                disabled={foundryBusy !== null || !foundryAvailable}
                onClick={() => void handlePrepareFoundry()}
              >
                {foundryPrepareLabel}
              </Btn>
              {foundryBusy === 'prepare' && (
                <Btn
                  variant="ghost"
                  size="sm"
                  disabled={foundryCancelRequested}
                  onClick={() => void handleCancelFoundryPrepare()}
                >
                  {foundryCancelRequested
                    ? t('localAsr.foundryCancelRequested')
                    : t('localAsr.foundryCancelPrepare')}
                </Btn>
              )}
              <Btn
                variant="ghost"
                size="sm"
                disabled={foundryBusy !== null || !foundryStatus?.loadedModelId}
                onClick={() => void handleReleaseFoundry()}
              >
                {foundryBusy === 'release'
                  ? t('localAsr.foundryReleasing')
                  : t('localAsr.releaseNow')}
              </Btn>
              <Btn
                variant="ghost"
                size="sm"
                disabled={foundryBusy !== null}
                onClick={() => void handleRevealFoundryDir()}
              >
                {foundryBusy === 'reveal' ? t('common.loading') : t('localAsr.revealDir')}
              </Btn>
              <Btn
                variant="ghost"
                size="sm"
                disabled={foundryBusy !== null}
                onClick={() => void handleDeleteFoundry()}
              >
                {foundryBusy === 'delete' ? t('common.loading') : t('localAsr.delete')}
              </Btn>
            </div>
          </div>
        </Card>
      )}

      {IS_WINDOWS && (
        <Card style={{ marginBottom: 16 }}>
          <div
            ref={sherpaAnchorRef}
            onMouseDownCapture={activateScrollGuard}
            onKeyDownCapture={(event) => {
              if (event.key === 'Enter' || event.key === ' ') {
                activateScrollGuard();
              }
            }}
            style={{
              display: 'flex',
              flexDirection: 'column',
              gap: 14,
            }}
          >
            <div
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                gap: 16,
                flexWrap: 'wrap',
              }}
            >
              <div style={{ minWidth: 0, flex: '1 1 360px' }}>
                <div
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 8,
                    marginBottom: 6,
                    flexWrap: 'wrap',
                  }}
                >
                  <div
                    style={{
                      fontSize: 14,
                      fontWeight: 700,
                      color: 'var(--ol-ink)',
                    }}
                  >
                    {t('localAsr.sherpaTitle')}
                  </div>
                  {sherpaDefault && (
                    <Pill tone="blue" size="sm">
                      {t('localAsr.activeBadge')}
                    </Pill>
                  )}
                  <Pill tone={sherpaStatus?.available ? 'ok' : 'outline'} size="sm">
                    {sherpaStatus?.available
                      ? t('localAsr.foundryAvailable')
                      : t('localAsr.foundryUnavailable')}
                  </Pill>
                  <Pill tone={sherpaStatus?.runtimeReady ? 'ok' : 'outline'} size="sm">
                    {sherpaStatus?.runtimeReady
                      ? t('localAsr.sherpaRuntimeReady')
                      : t('localAsr.sherpaRuntimeMissing')}
                  </Pill>
                </div>
                <div
                  style={{
                    fontSize: 13,
                    color: 'var(--ol-ink-3)',
                    lineHeight: 1.55,
                  }}
                >
                  {t('localAsr.sherpaDesc')}
                </div>
              </div>
              <div
                style={{
                  display: 'flex',
                  gap: 10,
                  flexWrap: 'wrap',
                  justifyContent: 'flex-end',
                }}
              >
                <label
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 4,
                    fontSize: 11,
                    color: 'var(--ol-ink-4)',
                  }}
                >
                  {t('localAsr.foundrySelectedModel')}
                  <SelectLite
                    value={selectedSherpaAlias}
                    onChange={(value) => {
                      void handleSherpaModelChange(value as SherpaOnnxModelAlias);
                    }}
                    disabled={sherpaBusy !== null}
                    options={sherpaModelOptions}
                    ariaLabel={t('localAsr.foundrySelectedModel')}
                    style={{
                      fontSize: 13,
                      height: 31,
                      padding: '0 10px',
                      borderRadius: 8,
                      border: '0.5px solid rgba(0,0,0,0.12)',
                      background: 'var(--ol-surface)',
                      color: 'var(--ol-ink)',
                      minWidth: 260,
                    }}
                  />
                </label>
                <label
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 4,
                    fontSize: 11,
                    color: 'var(--ol-ink-4)',
                  }}
                >
                  {t('localAsr.foundryLanguageLabel')}
                  <SelectLite
                    value={selectedSherpaLanguageHint}
                    onChange={(value) => {
                      activateScrollGuard();
                      void handleSherpaLanguageChange(value as SherpaOnnxLanguageHint);
                    }}
                    disabled={sherpaBusy !== null}
                    options={sherpaLanguageOptions}
                    ariaLabel={t('localAsr.foundryLanguageLabel')}
                    style={{
                      fontSize: 13,
                      height: 31,
                      padding: '0 10px',
                      borderRadius: 8,
                      border: '0.5px solid rgba(0,0,0,0.12)',
                      background: 'var(--ol-surface)',
                      color: 'var(--ol-ink)',
                      minWidth: 132,
                    }}
                  />
                </label>
                <label
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 4,
                    fontSize: 11,
                    color: 'var(--ol-ink-4)',
                  }}
                >
                  {t('localAsr.mirrorLabel')}

                  <SelectLite
                    value={selectedSherpaMirrorValue}
                    onChange={(v) => void handleMirrorChange(v)}
                    disabled={sherpaBusy !== null || selectedSherpaUsesReleaseArchive}
                    ariaLabel={t('localAsr.mirrorLabel')}
                    options={
                      selectedSherpaUsesReleaseArchive
                        ? [
                            {
                              value: 'github-release',
                              label: t('localAsr.mirrorGithubRelease'),
                            },
                          ]
                        : [
                            {
                              value: 'huggingface',
                              label: t('localAsr.mirrorHuggingface'),
                            },
                            {
                              value: 'hf-mirror',
                              label: t('localAsr.mirrorHfMirror'),
                            },
                          ]
                    }
                    style={{
                      fontSize: 13,
                      height: 31,
                      minWidth: 200,
                    }}
                  />
                </label>
              </div>
            </div>

            <div
              style={{
                fontSize: 12.5,
                color: 'var(--ol-ink-3)',
                lineHeight: 1.6,
              }}
            >
              <div>
                <span style={{ color: 'var(--ol-ink-4)' }}>
                  {t('localAsr.foundrySelectedModel')}:{' '}
                </span>
                <strong>{selectedSherpaDisplayName}</strong>
                <span>
                  {' '}
                  · {selectedSherpaSizeLabel} · {selectedSherpaDownloadLabel}
                </span>
                <span> · {t(selectedSherpaModel.descKey)}</span>
              </div>
              <div>
                <span style={{ color: 'var(--ol-ink-4)' }}>{t('localAsr.sherpaModelDir')}: </span>
                <code>{sherpaModelDir || '—'}</code>
              </div>
              <div>
                <span style={{ color: 'var(--ol-ink-4)' }}>
                  {t('localAsr.foundryLoadedModel')}:{' '}
                </span>
                {sherpaStatus?.loadedModelId ?? t('localAsr.foundryNotLoaded')}
              </div>
              {sherpaStatus?.error && (
                <div style={{ color: '#9b2c2c' }}>
                  <span>{t('localAsr.sherpaError')}: </span>
                  {sherpaStatus.error}
                </div>
              )}
            </div>

            {(sherpaBusy === 'prepare' || sherpaProgress) && (
              <FoundryPrepareProgressBlock
                progress={sherpaProgress}
                modelCached={selectedSherpaCatalog?.cached === true}
                cancelRequested={sherpaCancelRequested}
              />
            )}

            {showSherpaDownloadProgress && (
              <DownloadProgressBlock
                progress={selectedSherpaDownloadProgressForDisplay}
                remoteSize={selectedSherpaRemoteSize}
                cancelRequested={sherpaDownloadCancelRequested}
              />
            )}

            <div
              style={{
                display: 'flex',
                gap: 8,
                flexWrap: 'wrap',
              }}
            >
              <Btn
                variant="blue"
                size="sm"
                disabled={sherpaBusy !== null || !sherpaAvailable}
                onClick={() => void handleEnableSherpa()}
              >
                {sherpaBusy === 'enable'
                  ? t('localAsr.foundryEnabling')
                  : t('localAsr.sherpaSetDefault')}
              </Btn>
              <Btn
                variant="primary"
                size="sm"
                disabled={sherpaBusy !== null || !sherpaAvailable}
                onClick={() => void handlePrepareSherpa()}
              >
                {sherpaPrepareLabel}
              </Btn>
              {selectedSherpaCatalog?.cached !== true && !isSherpaDownloading && (
                <Btn
                  variant="primary"
                  size="sm"
                  disabled={sherpaBusy !== null || !sherpaAvailable}
                  onClick={() => void handleDownloadSherpa()}
                >
                  {hasSherpaPartial ? t('localAsr.resume') : t('localAsr.download')}
                </Btn>
              )}
              {isSherpaDownloading && (
                <Btn
                  variant="ghost"
                  size="sm"
                  disabled={sherpaDownloadCancelRequested}
                  onClick={() => void handleCancelSherpaDownload()}
                >
                  {sherpaDownloadCancelRequested
                    ? t('localAsr.foundryCancelRequested')
                    : t('localAsr.cancel')}
                </Btn>
              )}
              {sherpaBusy === 'prepare' && (
                <Btn
                  variant="ghost"
                  size="sm"
                  disabled={sherpaCancelRequested}
                  onClick={() => void handleCancelSherpaPrepare()}
                >
                  {sherpaCancelRequested
                    ? t('localAsr.foundryCancelRequested')
                    : t('localAsr.foundryCancelPrepare')}
                </Btn>
              )}
              <Btn
                variant="ghost"
                size="sm"
                disabled={sherpaBusy !== null || !sherpaStatus?.loadedModelId}
                onClick={() => void handleReleaseSherpa()}
              >
                {sherpaBusy === 'release'
                  ? t('localAsr.foundryReleasing')
                  : t('localAsr.releaseNow')}
              </Btn>
              <Btn
                variant="ghost"
                size="sm"
                disabled={sherpaBusy !== null}
                onClick={() => void handleRevealSherpaDir()}
              >
                {sherpaBusy === 'reveal' ? t('common.loading') : t('localAsr.sherpaRevealDir')}
              </Btn>
              <Btn
                variant="ghost"
                size="sm"
                disabled={sherpaBusy !== null || !canDeleteSelectedSherpa}
                onClick={() => void handleDeleteSherpa()}
              >
                {sherpaBusy === 'delete' ? t('common.loading') : t('localAsr.delete')}
              </Btn>
            </div>
          </div>
        </Card>
      )}

      {/* Qwen3 model management area — rendered on macOS only (the backend is #[cfg(target_os = "macos")] exclusive).
          On Windows / Linux the mirror / download / model list here would be dead UI. The Foundry block is already
          guarded by IS_WINDOWS above; the error Card (shared setError, also written by Foundry handlers) stays
          unconditionally visible. */}
      {IS_QWEN_PLATFORM && !engineAvailable && (
        <Card
          style={{
            marginBottom: 16,
            background: 'var(--ol-surface-2)',
          }}
        >
          <div
            style={{
              fontSize: 13,
              color: 'var(--ol-ink-2)',
            }}
          >
            {t('localAsr.engineUnavailable')}
          </div>
        </Card>
      )}
    </LocalAsrContentWrapper>
  );
}

// Presentational sub-components (FoundryPrepareProgressBlock, DownloadProgressBlock,
// ModelRow, TestResultBlock) live in ./components — imported at the top of this file.

// Pure UI helpers (alias/language-hint guards, platform detection, size
// formatting) live in ./helpers — imported at the top of this file.
