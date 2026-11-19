import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type {
  MarketplaceDetail,
  MarketplaceListItem,
  MarketplaceMyPackItem,
  StylePack,
} from '../../api/contracts';
import {
  fetchMarketplaceDetail,
  likeMarketplacePack,
  listMarketplace,
  marketplaceAuthStatus,
  marketplaceMyLikes,
  marketplaceMyPacks,
  readMarketplaceDetailCache,
  readMarketplaceListCache,
  uploadMarketplacePack,
  writeMarketplaceDetailCache,
  writeMarketplaceListCache,
} from '../../api/marketplace';
import { listStylePacks } from '../../api/style-packs';
import { useHotkeySettings } from '../../data/preferences';
import {
  canStartMarketplaceInstall,
  shouldCloseMarketplaceDetail,
  type MarketplaceInstallError,
} from '../../lib/marketplaceInstall';
import {
  readCachedLikes,
  reduceMarketplaceLikes,
  writeCachedLikes,
} from '../../lib/marketplaceLikes';
import { useConservativeLayout, useLayoutStack, useMobileLayout } from '../../lib/useMobileLayout';
import { useInteraction } from '../../ui/overlays/InteractionProvider';
import { downloadListedPack, installListedPack, withdrawListedPack } from './package-actions';

type SortMode = 'popular' | 'new' | 'liked';

export function useMarketplaceWorkspace() {
  const { t } = useTranslation();
  const { confirm, notify } = useInteraction('marketplace');
  const mobile = useMobileLayout();
  const baseLayoutStack = useLayoutStack();
  const conservative = useConservativeLayout();
  const stackLayout = conservative || baseLayoutStack;
  const { prefs, updatePrefs } = useHotkeySettings();

  // A fresh cached default list can be used immediately. Returning to it after
  // a search, an explicit retry, or a mutation fetches an authoritative list.
  const [items, setItems] = useState<MarketplaceListItem[]>(() => readMarketplaceListCache() ?? []);
  const [loading, setLoading] = useState(() => items.length === 0);
  const cachedEntry = useRef(items.length > 0);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [debouncedQuery, setDebouncedQuery] = useState('');
  const [sort, setSort] = useState<SortMode>('popular');
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<MarketplaceDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const setActionMsg = useCallback(
    (message: { kind: 'ok' | 'err'; text: string }) => {
      notify({
        key: 'marketplace-action',
        state: message.kind === 'ok' ? 'saved' : 'failed',
        message: message.text,
        durationMs: message.kind === 'ok' ? 2500 : 6000,
      });
    },
    [notify],
  );
  const [installingPackId, setInstallingPackId] = useState<string | null>(null);
  const [downloadingPackId, setDownloadingPackId] = useState<string | null>(null);
  const [installError, setInstallError] = useState<MarketplaceInstallError | null>(null);

  const [showUpload, setShowUpload] = useState(false);
  const [uploadOriginPackId, setUploadOriginPackId] = useState<string | null>(null);
  const [uploadTargetName, setUploadTargetName] = useState<string | null>(null);
  const [localPacks, setLocalPacks] = useState<StylePack[]>([]);
  // Upload picker selection: clicking a pack card selects it (no immediate upload); the bottom "confirm upload" submits.
  const [selectedUploadPackId, setSelectedUploadPackId] = useState<string | null>(null);
  const [myPacks, setMyPacks] = useState<MarketplaceMyPackItem[]>([]);
  // "My packs" is dialog-shaped: showMyPacks toggles it, myPacksQuery is a dialog-local search term
  // (kept separate from the outer marketplace search query).
  const [showMyPacks, setShowMyPacks] = useState(false);
  const [myPacksQuery, setMyPacksQuery] = useState('');
  // Loading/error/success tri-state: loading (first fetch or retry), error (HTTP failure / parse failure), success (default).
  // The old version only had success + toast, which caused: while fetching it showed "you haven't published any
  // style packs", misleading users; failures only toasted with no inline retry entry.
  const [myPacksLoading, setMyPacksLoading] = useState(false);
  const [myPacksError, setMyPacksError] = useState<string | null>(null);
  // GitHub login dialog toggle — the login flow is handled by the shared <GithubLoginModal />.
  const [showLogin, setShowLogin] = useState(false);
  // Set of pack ids the user has liked — drives heart rendering + the "liked" filter.
  // Fetched once on entering the marketplace; mutated locally after starring.
  const [likedIds, setLikedIds] = useState<Set<string>>(new Set());
  const [likesSettled, setLikesSettled] = useState(false);
  const likesSeqRef = useRef(0);
  const currentLogin = (prefs?.marketplaceDevLogin ?? '').trim();
  const [marketplaceSignedIn, setMarketplaceSignedIn] = useState(false);
  const authorizedLogin = marketplaceSignedIn ? currentLogin : '';
  const canUpload = marketplaceSignedIn;
  const refreshAuthStatus = useCallback(async () => {
    try {
      const status = await marketplaceAuthStatus();
      setMarketplaceSignedIn(status.signedIn);
      if (!status.signedIn) {
        setLikedIds((current) => reduceMarketplaceLikes(current, { type: 'signed-out' }));
        setLikesSettled(true);
        setMyPacks([]);
        if (currentLogin) {
          await updatePrefs((current) => ({ ...current, marketplaceDevLogin: '' }));
        }
      }
      return status.signedIn;
    } catch (error) {
      // A thrown status read is not a sign-out. Treating it as one cleared the
      // heart the next time the marketplace effect restarted.
      console.warn('[marketplace] auth status failed', error);
      setLikedIds((current) => reduceMarketplaceLikes(current, { type: 'status-failed' }));
      return false;
    }
  }, [currentLogin, updatePrefs]);
  // "Derived from" shows only when the origin author != the current login — don't tag a user's own pack as derivative of themselves.
  const isDerivative = (originLogin: string | null | undefined): boolean =>
    !!originLogin && originLogin !== currentLogin;

  // 300ms search debounce
  useEffect(() => {
    const id = window.setTimeout(() => setDebouncedQuery(query), 300);
    return () => window.clearTimeout(id);
  }, [query]);

  // Monotonic seq guards against stale responses overwriting fresh state: when the user quickly edits the
  // query / switches packs, an old request's response may arrive after the new one; compare seq and drop stale results.
  const reqSeqRef = useRef(0);
  const detailSeqRef = useRef(0);
  const refresh = useCallback(
    async (notify = false) => {
      const seq = ++reqSeqRef.current;
      setLoading(true);
      setLoadError(null);
      try {
        // The backend only understands popular/new — 'liked' fetches via popular and filters on the frontend.
        const serverSort: 'popular' | 'new' = sort === 'liked' ? 'popular' : sort;
        const list = await listMarketplace({ query: debouncedQuery, sort: serverSort, limit: 50 });
        if (seq !== reqSeqRef.current) return; // stale response
        setItems(list);
        // Cache only the "default view" (popular + empty query) so reopening is instant.
        if (serverSort === 'popular' && debouncedQuery.trim() === '') {
          writeMarketplaceListCache(list);
        }
        if (notify) {
          setActionMsg({ kind: 'ok', text: t('common.refreshed') });
        }
      } catch (error) {
        if (seq !== reqSeqRef.current) return;
        console.error('[marketplace] list failed', error);
        setLoadError(errorMessage(error));
        if (notify) {
          setActionMsg({ kind: 'err', text: t('common.operationFailed') });
        }
      } finally {
        if (seq === reqSeqRef.current) setLoading(false);
      }
    },
    [debouncedQuery, sort, setActionMsg, t],
  );

  const visibleItems = useMemo(() => {
    if (sort === 'liked') return items.filter((it) => likedIds.has(it.id));
    return items;
  }, [items, sort, likedIds]);

  const visibleMyPacks = useMemo(() => {
    // Hide withdrawn / superseded immediately:
    // - withdrawn: the user already took it down; a 5-minute grace window would make counts disagree
    //   (user report: published 1, showed 2). Withdraw feedback is published only after the response.
    // - superseded: server state of an old version after a new one is published; from the user's view
    //   the old version has been replaced and no longer counts as currently online in "my packs".
    const q = myPacksQuery.trim().toLowerCase();
    return myPacks.filter((pack) => {
      if (pack.state === 'withdrawn' || pack.state === 'superseded') return false;
      if (!q) return true;
      return (
        pack.name.toLowerCase().includes(q) ||
        pack.description.toLowerCase().includes(q) ||
        pack.tags.some((tag) => tag.toLowerCase().includes(q))
      );
    });
  }, [myPacks, myPacksQuery]);

  useEffect(() => {
    if (cachedEntry.current && sort === 'popular' && !debouncedQuery.trim()) return;
    cachedEntry.current = false;
    void refresh();
  }, [refresh, sort, debouncedQuery]);

  useEffect(() => {
    void refreshAuthStatus();
  }, [currentLogin, refreshAuthStatus]);

  // Fetch likes when the signed-in identity changes. A newer like click bumps
  // likesSeqRef so a slow list response cannot wipe the heart that just landed.
  useEffect(() => {
    if (!marketplaceSignedIn) {
      setLikedIds((current) => reduceMarketplaceLikes(current, { type: 'signed-out' }));
      setLikesSettled(true);
      return;
    }
    const cached = readCachedLikes(currentLogin);
    if (cached.length > 0) {
      setLikedIds(new Set(cached));
      setLikesSettled(true);
    } else {
      setLikesSettled(false);
    }
    const seq = ++likesSeqRef.current;
    let cancelled = false;
    void (async () => {
      try {
        const ids = await marketplaceMyLikes();
        if (cancelled || seq !== likesSeqRef.current) return;
        const next = reduceMarketplaceLikes(new Set(), { type: 'fetched', ids });
        setLikedIds(next);
        writeCachedLikes(currentLogin, next);
        setLikesSettled(true);
      } catch (error) {
        if (cancelled || seq !== likesSeqRef.current) return;
        console.warn('[marketplace] fetch my-likes failed', error);
        setLikedIds((current) => reduceMarketplaceLikes(current, { type: 'fetch-failed' }));
        setLikesSettled(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [marketplaceSignedIn, currentLogin]);

  const myPacksSequence = useRef(0);
  const refreshMyPacks = useCallback(
    async (notify = false) => {
      const sequence = ++myPacksSequence.current;
      if (!marketplaceSignedIn) {
        setMyPacks([]);
        setMyPacksLoading(false);
        setMyPacksError(null);
        return;
      }
      setMyPacksLoading(true);
      setMyPacksError(null);
      try {
        const packs = await marketplaceMyPacks();
        if (sequence !== myPacksSequence.current) return;
        setMyPacks(packs);
        if (notify) {
          setActionMsg({ kind: 'ok', text: t('common.refreshed') });
        }
      } catch (error) {
        if (sequence !== myPacksSequence.current) return;
        void refreshAuthStatus();
        console.warn('[marketplace] fetch my-packs failed', error);
        const msg = errorMessage(error);
        setMyPacksError(msg);
        // Still toast for behavior compatibility; the inline error lets users retry directly in the dialog.
        setActionMsg({ kind: 'err', text: t('marketplace.myPacks.loadFailed', { err: msg }) });
      } finally {
        if (sequence === myPacksSequence.current) setMyPacksLoading(false);
      }
    },
    [marketplaceSignedIn, refreshAuthStatus, t],
  );

  useEffect(() => {
    void refreshMyPacks();
  }, [refreshMyPacks]);

  // Refresh "my packs" once when the dialog opens so it doesn't show stale data.
  useEffect(() => {
    if (showMyPacks && marketplaceSignedIn) {
      void refreshMyPacks();
    }
  }, [showMyPacks, marketplaceSignedIn, refreshMyPacks]);

  const openDetail = async (id: string) => {
    const seq = ++detailSeqRef.current;
    setSelectedId(id);
    setInstallError((previous) => (previous?.packId === id ? previous : null));
    // Differential cache hit: the list already carries version+updatedAt; match the local detail by the triple.
    // Hit = render directly, skip the network; miss = go through fetchMarketplaceDetail.
    // Keep a previous detail mounted so the heart does not unmount into a spinner on every open.
    const listItem = items.find((it) => it.id === id);
    const cached = listItem
      ? readMarketplaceDetailCache(id, listItem.version ?? '', listItem.updatedAt ?? '')
      : null;
    if (cached) {
      if (seq === detailSeqRef.current) {
        setDetail(cached);
        setDetailLoading(false);
      }
      return;
    }
    if (detail?.id !== id) {
      setDetail(null);
      setDetailLoading(true);
    }
    try {
      const d = await fetchMarketplaceDetail(id);
      if (seq !== detailSeqRef.current) return; // stale: user already switched to another pack
      // Validate then write back: writeMarketplaceDetailCache checks ID / size.
      writeMarketplaceDetailCache(d);
      setDetail(d);
    } catch (error) {
      if (seq !== detailSeqRef.current) return;
      console.error('[marketplace] detail failed', error);
      setActionMsg({
        kind: 'err',
        text: t('marketplace.errors.detail', { err: errorMessage(error) }),
      });
      // A list row is enough to keep the heart on screen. Closing the dialog made it vanish.
      if (!listItem) setSelectedId(null);
    } finally {
      if (seq === detailSeqRef.current) setDetailLoading(false);
    }
  };

  const refreshRef = useRef(refresh);
  refreshRef.current = refresh;
  const closeDetail = () => {
    detailSeqRef.current++;
    setSelectedId(null);
    setInstallError(null);
    cachedEntry.current = false;
    void refresh();
  };

  const onInstall = async () => {
    if (!detail || !canStartMarketplaceInstall(installingPackId)) return;
    const packId = detail.id;
    const packName = detail.name;
    setInstallingPackId(packId);
    setInstallError(null);
    try {
      await installListedPack(packId);
      setActionMsg({ kind: 'ok', text: t('marketplace.installed', { name: packName }) });
      setSelectedId((current) => (shouldCloseMarketplaceDetail(current, packId) ? null : current));
      cachedEntry.current = false;
      void refreshRef.current();
    } catch (error) {
      const errorText = errorMessage(error);
      const message = t('marketplace.errors.install', { err: errorText });
      setInstallError({ packId, message });
      setActionMsg({ kind: 'err', text: message });
    } finally {
      setInstallingPackId((current) => (current === packId ? null : current));
    }
  };

  const onDownload = async (pack: MarketplaceListItem) => {
    if (downloadingPackId !== null) return;
    setDownloadingPackId(pack.id);
    try {
      if (!(await downloadListedPack(pack))) return;
      setActionMsg({ kind: 'ok', text: t('marketplace.downloaded', { name: pack.name }) });
    } catch (error) {
      setActionMsg({
        kind: 'err',
        text: t('marketplace.errors.download', { err: errorMessage(error) }),
      });
    } finally {
      setDownloadingPackId((current) => (current === pack.id ? null : current));
    }
  };

  const onLike = async (packId: string, likeCount: number) => {
    if (!marketplaceSignedIn) {
      setActionMsg({ kind: 'err', text: t('marketplace.myPacks.notLoggedIn') });
      setShowLogin(true);
      return;
    }
    const prevLikedIds = likedIds;
    const prevLikeCount = likeCount;
    const wasLiked = prevLikedIds.has(packId);
    // Optimistic mutate: flip the heart and adjust the count immediately so the click feels instant.
    // Bump the likes seq so an in-flight "my likes" response cannot put the heart back.
    const optimisticCount = Math.max(0, prevLikeCount + (wasLiked ? -1 : 1));
    likesSeqRef.current += 1;
    setLikedIds((prev) => {
      const next = new Set(prev);
      if (wasLiked) next.delete(packId);
      else next.add(packId);
      if (currentLogin) writeCachedLikes(currentLogin, next);
      return next;
    });
    setDetail((prev) =>
      prev && prev.id === packId ? { ...prev, likeCount: optimisticCount } : prev,
    );
    setItems((prev) =>
      prev.map((p) => (p.id === packId ? { ...p, likeCount: optimisticCount } : p)),
    );
    try {
      const r = await likeMarketplacePack(packId);
      // After the server responds, recalibrate to the server's likeCount / alreadyLiked (guards against concurrency or local drift).
      setDetail((prev) =>
        prev && prev.id === packId ? { ...prev, likeCount: r.likeCount } : prev,
      );
      setItems((prev) => prev.map((p) => (p.id === packId ? { ...p, likeCount: r.likeCount } : p)));
      setLikedIds((prev) => {
        const next = new Set(prev);
        if (r.alreadyLiked) next.add(packId);
        else next.delete(packId);
        if (currentLogin) writeCachedLikes(currentLogin, next);
        return next;
      });
    } catch (error) {
      void refreshAuthStatus();
      // Roll back to the pre-click state
      likesSeqRef.current += 1;
      setLikedIds(prevLikedIds);
      if (currentLogin) writeCachedLikes(currentLogin, prevLikedIds);
      setDetail((prev) =>
        prev && prev.id === packId ? { ...prev, likeCount: prevLikeCount } : prev,
      );
      setItems((prev) =>
        prev.map((p) => (p.id === packId ? { ...p, likeCount: prevLikeCount } : p)),
      );
      setActionMsg({
        kind: 'err',
        text: t('marketplace.errors.like', { err: errorMessage(error) }),
      });
    }
  };

  const openUploadPicker = async (
    originPackId: string | null = null,
    targetName: string | null = null,
  ) => {
    try {
      setUploadOriginPackId(originPackId);
      setUploadTargetName(targetName);
      const packs = await listStylePacks();
      // Builtin packs are read-only templates and cannot be uploaded; when updating, sort the same-named local version to the front.
      const target = (targetName ?? '').trim().toLowerCase();
      const editable = packs
        .filter((p) => p.kind !== 'builtin')
        .sort((a, b) => {
          const aMatch = target.length > 0 && a.name.trim().toLowerCase() === target;
          const bMatch = target.length > 0 && b.name.trim().toLowerCase() === target;
          if (aMatch !== bMatch) return aMatch ? -1 : 1;
          return a.name.localeCompare(b.name);
        });
      setLocalPacks(editable);
      // In the update flow, preselect the "suggested update" local pack (same name) so the user usually just confirms.
      const recommended =
        target.length > 0
          ? editable.find((p) => p.name.trim().toLowerCase() === target)
          : undefined;
      setSelectedUploadPackId(recommended?.id ?? null);
      setShowUpload(true);
    } catch (error) {
      setActionMsg({
        kind: 'err',
        text: t('marketplace.errors.loadLocal', { err: errorMessage(error) }),
      });
    }
  };

  const currentOwner = useRef(currentLogin);
  currentOwner.current = currentLogin;
  const withdrawing = useRef(false);
  const withdraw = async (pack: { id: string; name: string; authorLogin: string }) => {
    if (withdrawing.current || pack.authorLogin !== currentOwner.current) return;
    withdrawing.current = true;
    try {
      await confirm({
        title: t('marketplace.detail.withdrawBtn'),
        description: t('marketplace.detail.withdrawScope', { name: pack.name }),
        tone: 'danger',
        confirmLabel: t('marketplace.detail.withdrawBtn'),
        icon: 'trash',
        errorMessage: t('marketplace.detail.withdrawFailed', { err: t('common.operationFailed') }),
        onConfirm: async () => {
          try {
            await withdrawListedPack(pack, () => currentOwner.current);
          } catch (error) {
            void refreshAuthStatus();
            throw error;
          }
          ++reqSeqRef.current;
          ++myPacksSequence.current;
          setItems((current) => current.filter((item) => item.id !== pack.id));
          setMyPacks((current) => current.filter((item) => item.id !== pack.id));
          setSelectedId((current) => (current === pack.id ? null : current));
          cachedEntry.current = false;
          notify({
            key: 'marketplace-action',
            state: 'deleted',
            message: t('marketplace.detail.withdrawSuccess'),
          });
          void refreshRef.current();
          void refreshMyPacks();
        },
      });
    } finally {
      withdrawing.current = false;
    }
  };

  const onDelete = async () => {
    if (detail) await withdraw(detail);
  };

  const onDeleteMine = withdraw;

  const onUpload = async (packId: string) => {
    const localPack = localPacks.find((p) => p.id === packId);
    try {
      const result = await uploadMarketplacePack(packId, uploadOriginPackId);
      // Optimistic: on 200, push this pack to the top of "my packs" with the backend's returned state (usually 'pending').
      // Avoids waiting 1.5s / 5s polling to see it — later polling overwrites with real server data.
      if (localPack && currentLogin) {
        const nowIso = new Date().toISOString();
        const optimistic: MarketplaceMyPackItem = {
          id: result.id,
          slug: '',
          name: localPack.name,
          description: localPack.description ?? '',
          authorLogin: currentLogin,
          version: localPack.version ?? '',
          baseMode: localPack.baseMode ?? 'structured',
          tags: localPack.tags ?? [],
          likeCount: 0,
          downloadCount: 0,
          publishedAt: nowIso,
          updatedAt: nowIso,
          originPackId: uploadOriginPackId ?? null,
          originAuthorLogin: null,
          state: result.state,
        };
        setMyPacks((prev) => {
          const idx = prev.findIndex((p) => p.id === result.id);
          if (idx >= 0) {
            // Same-id update by the original author: keep server counters like likes/downloads, overwrite meta
            // and reset state to pending.
            const next = [...prev];
            next[idx] = {
              ...next[idx],
              name: optimistic.name,
              description: optimistic.description,
              version: optimistic.version,
              baseMode: optimistic.baseMode,
              tags: optimistic.tags,
              updatedAt: nowIso,
              state: result.state,
            };
            return next;
          }
          return [optimistic, ...prev];
        });
      }
      setActionMsg({ kind: 'ok', text: t('marketplace.uploaded') });
      setShowUpload(false);
      setUploadOriginPackId(null);
      setUploadTargetName(null);
      setSelectedUploadPackId(null);
      // issue #470: after upload, give the backend time to persist and run review, then recalibrate once with
      // real server data (review state may go pending→approved/rejected). The optimistic update already reflects
      // "my packs" instantly, so a single catch-up refresh suffices; use the longer delay (5s) to ensure
      // eventual consistency, dropping the redundant 1.5s pass.
      window.setTimeout(() => {
        cachedEntry.current = false;
        void refreshRef.current();
        void refreshMyPacks();
      }, 5000);
    } catch (error) {
      void refreshAuthStatus();
      setActionMsg({
        kind: 'err',
        text: t('marketplace.errors.upload', { err: errorMessage(error) }),
      });
    }
  };

  // After a successful GitHub login Rust has already saved the token; prefs caches only the login for display.
  const onLoginSuccess = useCallback(
    (nextLogin: string) => {
      setMarketplaceSignedIn(true);
      // A failed prefs write is only logged (same as the pre-refactor OAuth polling) — never bare void,
      // or the rejection surfaces as an unhandled promise rejection.
      void updatePrefs((current) => ({ ...current, marketplaceDevLogin: nextLogin })).catch((e) =>
        console.warn('[marketplace] save login to prefs failed', e),
      );
      setActionMsg({ kind: 'ok', text: t('marketplace.oauth.successAs', { login: nextLogin }) });
    },
    [updatePrefs, t],
  );

  const sortPills = useMemo<Array<{ id: SortMode; label: string }>>(
    () => [
      { id: 'popular', label: t('marketplace.sortPopular') },
      { id: 'new', label: t('marketplace.sortNew') },
      { id: 'liked', label: t('marketplace.sortLiked') },
    ],
    [t],
  );

  return {
    t,
    mobile,
    stackLayout,
    prefs,
    items,
    loading,
    loadError,
    query,
    setQuery,
    sort,
    setSort,
    selectedId,
    closeDetail,
    detail,
    detailLoading,
    installingPackId,
    downloadingPackId,
    installError,
    setInstallError,
    showUpload,
    setShowUpload,
    uploadOriginPackId,
    setUploadOriginPackId,
    uploadTargetName,
    setUploadTargetName,
    localPacks,
    selectedUploadPackId,
    setSelectedUploadPackId,
    myPacks,
    showMyPacks,
    setShowMyPacks,
    myPacksQuery,
    setMyPacksQuery,
    myPacksLoading,
    myPacksError,
    showLogin,
    setShowLogin,
    likedIds,
    likesSettled,
    currentLogin,
    marketplaceSignedIn,
    authorizedLogin,
    canUpload,
    isDerivative,
    refresh,
    visibleItems,
    visibleMyPacks,
    refreshMyPacks,
    openDetail,
    onInstall,
    onDownload,
    onLike,
    openUploadPicker,
    onDelete,
    onDeleteMine,
    onUpload,
    onLoginSuccess,
    sortPills,
  };
}

function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
