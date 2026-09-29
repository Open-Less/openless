// Marketplace.tsx — Style Pack Marketplace browse panel.
//
// Phase A goals (goal 1.a-e):
//   (a) backend validation — talk to the backend via marketplace_* IPC
//   (b) upload and fetch features — Install / Upload buttons
//   (c) standalone dialog UI — modal-style detail card
//   (d) search box — top input + server-side ?q=
//   (e) rank-based auto-recommendation — default sort=popular
//
// The backend URL comes from prefs.marketplaceBaseUrl; dev defaults to http://127.0.0.1:8090;
// once the user fills the production URL in Settings the client switches automatically.
// The GitHub login is display only; write access is decided by the OAuth token in the Rust credentials vault.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import { Icon } from '../components/Icon';
import { SavedToast } from '../components/SavedToast';
import { ThinkingDots } from '../components/ThinkingDots';
import { GithubLoginModal } from '../components/GithubLoginModal';
import { PresenceModal } from '../components/ui/Modal';
import {
  downloadMarketplacePack,
  fetchMarketplaceDetail,
  installMarketplacePack,
  isTauri,
  likeMarketplacePack,
  listMarketplace,
  listStylePacks,
  logClientError,
  marketplaceDelete,
  marketplaceAuthStatus,
  marketplaceMyLikes,
  marketplaceMyPacks,
  readMarketplaceDetailCache,
  readMarketplaceListCache,
  uploadMarketplacePack,
  writeMarketplaceDetailCache,
  writeMarketplaceListCache,
} from '../lib/ipc';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import type {
  MarketplaceDetail,
  MarketplaceListItem,
  MarketplaceMyPackItem,
  StylePack,
} from '../lib/types';
import {
  canStartMarketplaceInstall,
  isMarketplaceInstallActive,
  isMarketplaceInstallErrorForPack,
  shouldCloseMarketplaceDetail,
  type MarketplaceInstallError,
} from '../lib/marketplaceInstall';
import { pickStylePackZipTargetPath, stylePackZipFileName } from '../lib/stylePackZip';
import { useMobileLayout, useLayoutStack, useConservativeLayout } from '../lib/useMobileLayout';
import { Btn, Card, PageHeader, Pill } from './_atoms';

type SortMode = 'popular' | 'new' | 'liked';

export function Marketplace() {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  const baseLayoutStack = useLayoutStack();
  const conservative = useConservativeLayout();
  const stackLayout = conservative || baseLayoutStack;
  const { prefs, updatePrefs } = useHotkeySettings();

  // Read the cache at startup: the last default view (popular + empty query) list renders instantly; a background refresh corrects it.
  const [items, setItems] = useState<MarketplaceListItem[]>(() => readMarketplaceListCache() ?? []);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [debouncedQuery, setDebouncedQuery] = useState('');
  const [sort, setSort] = useState<SortMode>('popular');
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<MarketplaceDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [actionMsg, setActionMsg] = useState<{ kind: 'ok' | 'err'; text: string } | null>(null);
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
  const currentLogin = (prefs?.marketplaceDevLogin ?? '').trim();
  const [marketplaceSignedIn, setMarketplaceSignedIn] = useState(false);
  const authorizedLogin = marketplaceSignedIn ? currentLogin : '';
  const canUpload = marketplaceSignedIn;
  const refreshAuthStatus = useCallback(async () => {
    try {
      const status = await marketplaceAuthStatus();
      setMarketplaceSignedIn(status.signedIn);
      if (!status.signedIn) {
        setLikedIds(new Set());
        setMyPacks([]);
        if (currentLogin) {
          await updatePrefs((current) => ({ ...current, marketplaceDevLogin: '' }));
        }
      }
      return status.signedIn;
    } catch {
      setMarketplaceSignedIn(false);
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
  const refresh = useCallback(async () => {
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
    } catch (error) {
      if (seq !== reqSeqRef.current) return;
      console.error('[marketplace] list failed', error);
      setLoadError(errorMessage(error));
    } finally {
      if (seq === reqSeqRef.current) setLoading(false);
    }
  }, [debouncedQuery, sort]);

  const visibleItems = useMemo(() => {
    if (sort === 'liked') return items.filter((it) => likedIds.has(it.id));
    return items;
  }, [items, sort, likedIds]);

  const visibleMyPacks = useMemo(() => {
    // Hide withdrawn / superseded immediately:
    // - withdrawn: the user already took it down; a 5-minute grace window would make counts disagree
    //   (user report: published 1, showed 2). Withdraw feedback goes through the actionMsg toast.
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
    void refresh();
  }, [refresh]);

  useEffect(() => {
    void refreshAuthStatus();
  }, [currentLogin, refreshAuthStatus]);

  // Fetch the "my likes" cache once to render hearts + the "liked" filter. Refetch when the login identity changes.
  useEffect(() => {
    let cancelled = false;
    if (!marketplaceSignedIn) {
      setLikedIds(new Set());
      return () => {
        cancelled = true;
      };
    }
    void (async () => {
      try {
        const ids = await marketplaceMyLikes();
        if (!cancelled) setLikedIds(new Set(ids));
      } catch (error) {
        void refreshAuthStatus();
        console.warn('[marketplace] fetch my-likes failed', error);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [marketplaceSignedIn, refreshAuthStatus]);

  const refreshMyPacks = useCallback(async () => {
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
      setMyPacks(packs);
    } catch (error) {
      void refreshAuthStatus();
      console.warn('[marketplace] fetch my-packs failed', error);
      const msg = errorMessage(error);
      setMyPacksError(msg);
      // Still toast for behavior compatibility; the inline error lets users retry directly in the dialog.
      setActionMsg({ kind: 'err', text: t('marketplace.myPacks.loadFailed', { err: msg }) });
    } finally {
      setMyPacksLoading(false);
    }
  }, [marketplaceSignedIn, refreshAuthStatus, t]);

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
    setDetail(null);
    setDetailLoading(true);
    setInstallError((previous) => (previous?.packId === id ? previous : null));
    // Differential cache hit: the list already carries version+updatedAt; match the local detail by the triple.
    // Hit = render directly, skip the network; miss = go through fetchMarketplaceDetail.
    const listItem = items.find((it) => it.id === id);
    if (listItem) {
      const cached = readMarketplaceDetailCache(
        id,
        listItem.version ?? '',
        listItem.updatedAt ?? '',
      );
      if (cached) {
        if (seq === detailSeqRef.current) {
          setDetail(cached);
          setDetailLoading(false);
        }
        return;
      }
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
      setSelectedId(null);
    } finally {
      if (seq === detailSeqRef.current) setDetailLoading(false);
    }
  };

  const onInstall = async () => {
    if (!detail || !canStartMarketplaceInstall(installingPackId)) return;
    const packId = detail.id;
    const packName = detail.name;
    setInstallingPackId(packId);
    setInstallError(null);
    let clientFailureLog: string | null = null;
    try {
      await installMarketplacePack(packId);
      setActionMsg({ kind: 'ok', text: t('marketplace.installed', { name: packName }) });
      setSelectedId((current) => (shouldCloseMarketplaceDetail(current, packId) ? null : current));
    } catch (error) {
      const errorText = errorMessage(error);
      const message = t('marketplace.errors.install', { err: errorText });
      setInstallError({ packId, message });
      setActionMsg({ kind: 'err', text: message });
      clientFailureLog = `[marketplace-install] stage=ipc-failed pack_id=${packId} error=${errorText}`;
    } finally {
      setInstallingPackId((current) => (current === packId ? null : current));
      if (clientFailureLog) void logClientError(clientFailureLog);
    }
  };

  const onDownload = async (pack: MarketplaceListItem) => {
    if (downloadingPackId !== null) return;
    setDownloadingPackId(pack.id);
    try {
      const defaultName = stylePackZipFileName(pack.name, pack.version);
      const targetPath = await pickStylePackZipTargetPath(defaultName, isTauri);
      if (!targetPath) return;
      await downloadMarketplacePack(pack.id, targetPath);
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

  const onLike = async () => {
    if (!detail) return;
    if (!marketplaceSignedIn) {
      setActionMsg({ kind: 'err', text: t('marketplace.myPacks.notLoggedIn') });
      setShowLogin(true);
      return;
    }
    const packId = detail.id;
    const prevLikedIds = likedIds;
    const prevLikeCount = detail.likeCount;
    const wasLiked = prevLikedIds.has(packId);
    // Optimistic mutate: flip the heart and adjust the count immediately so the click feels instant.
    const optimisticCount = Math.max(0, prevLikeCount + (wasLiked ? -1 : 1));
    setLikedIds((prev) => {
      const next = new Set(prev);
      if (wasLiked) next.delete(packId);
      else next.add(packId);
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
        return next;
      });
    } catch (error) {
      void refreshAuthStatus();
      // Roll back to the pre-click state
      setLikedIds(prevLikedIds);
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

  const onDelete = async () => {
    if (!detail) return;
    if (detail.authorLogin !== currentLogin) return; // only the author can delete
    // eslint-disable-next-line no-alert
    if (!window.confirm(t('marketplace.detail.withdrawConfirm', { name: detail.name }))) return;
    try {
      await marketplaceDelete(detail.id);
      setActionMsg({ kind: 'ok', text: t('marketplace.detail.withdrawSuccess') });
      setSelectedId(null);
      // Remove it from the list immediately after withdrawing, then request again to confirm
      setItems((prev) => prev.filter((p) => p.id !== detail.id));
      void refresh();
    } catch (error) {
      void refreshAuthStatus();
      setActionMsg({
        kind: 'err',
        text: t('marketplace.detail.withdrawFailed', { err: errorMessage(error) }),
      });
    }
  };

  const onDeleteMine = async (pack: MarketplaceMyPackItem) => {
    if (pack.authorLogin !== currentLogin) return;
    // eslint-disable-next-line no-alert
    if (!window.confirm(t('marketplace.detail.withdrawConfirm', { name: pack.name }))) return;
    try {
      await marketplaceDelete(pack.id);
      setActionMsg({ kind: 'ok', text: t('marketplace.detail.withdrawSuccess') });
      setMyPacks((prev) => prev.filter((p) => p.id !== pack.id));
      setItems((prev) => prev.filter((p) => p.id !== pack.id));
      void refreshMyPacks();
    } catch (error) {
      void refreshAuthStatus();
      setActionMsg({
        kind: 'err',
        text: t('marketplace.detail.withdrawFailed', { err: errorMessage(error) }),
      });
    }
  };

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
        void refresh();
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

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        minHeight: 0,
        position: 'relative',
      }}
    >
      <PageHeader
        kicker={t('marketplace.kicker')}
        title={t('marketplace.title')}
        desc={t('marketplace.desc')}
        right={
          <div
            style={{
              display: 'flex',
              gap: 8,
              alignItems: 'center',
              flexWrap: 'wrap',
              justifyContent: 'flex-end',
            }}
          >
            <button
              type="button"
              onClick={() => setShowMyPacks(true)}
              title={
                authorizedLogin
                  ? t('marketplace.myPacks.buttonTitle', { login: authorizedLogin })
                  : t('marketplace.myPacks.buttonTitleEmpty')
              }
              style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: 8,
                height: 30,
                padding: '0 12px',
                borderRadius: 9,
                border: '0.5px solid var(--ol-line-strong)',
                background: 'var(--ol-surface)',
                color: 'var(--ol-ink-2)',
                fontSize: 12,
                fontWeight: 650,
                cursor: 'pointer',
                boxShadow: '0 1px 2px rgba(15,17,22,0.04)',
              }}
            >
              <span
                style={{
                  width: 18,
                  height: 18,
                  borderRadius: 999,
                  display: 'inline-grid',
                  placeItems: 'center',
                  background: 'var(--ol-surface-2)',
                  fontSize: 10,
                  fontWeight: 750,
                }}
              >
                {(authorizedLogin || '?').slice(0, 1).toUpperCase()}
              </span>
              <span>{t('marketplace.myPacks.buttonLabel')}</span>
            </button>
            <Btn icon="refresh" variant="ghost" size="sm" onClick={() => void refresh()}>
              {t('common.refresh')}
            </Btn>
          </div>
        }
      />

      {/* Top: search + sort */}
      <div
        className="ol-flex-row"
        style={{
          display: 'flex',
          gap: 10,
          alignItems: stackLayout ? 'stretch' : 'center',
          flexDirection: stackLayout ? 'column' : 'row',
          padding: '4px 0 14px',
        }}
      >
        <div
          style={{
            flex: 1,
            display: 'flex',
            alignItems: 'center',
            gap: 6,
            padding: '6px 10px',
            border: '0.5px solid var(--ol-line-strong)',
            borderRadius: 10,
            background: 'var(--ol-surface)',
          }}
        >
          <Icon name="search" size={14} stroke="var(--ol-ink-3)" />
          <input
            type="search"
            placeholder={t('marketplace.searchPlaceholder')}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            style={{
              flex: 1,
              outline: 'none',
              border: 0,
              background: 'transparent',
              fontSize: 13,
              color: 'var(--ol-ink-1)',
            }}
          />
        </div>
        <div
          className="ol-flex-row ol-flex-split"
          style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}
        >
          {sortPills.map((p) => (
            <button
              key={p.id}
              onClick={() => setSort(p.id)}
              style={{
                padding: '6px 10px',
                fontSize: 12,
                border: '0.5px solid var(--ol-line-strong)',
                borderRadius: 8,
                cursor: 'pointer',
                background: sort === p.id ? 'var(--ol-blue-soft)' : 'var(--ol-surface)',
                color: sort === p.id ? 'var(--ol-blue)' : 'var(--ol-ink-2)',
              }}
            >
              {p.label}
            </button>
          ))}
        </div>
      </div>

      {actionMsg && (
        <SavedToast
          saveState={actionMsg.kind === 'ok' ? 'saved' : 'failed'}
          message={actionMsg.text}
        />
      )}

      {loadError && (
        <Card padding={16} style={{ marginBottom: 12, borderColor: 'var(--ol-err)' }}>
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              gap: 10,
            }}
          >
            <div style={{ fontSize: 12, color: 'var(--ol-err)', flex: 1, wordBreak: 'break-word' }}>
              {t('marketplace.loadFailed', { err: loadError })}
            </div>
            <Btn variant="blue" size="sm" onClick={() => void refresh()}>
              {t('common.retry') ?? '重试'}
            </Btn>
          </div>
        </Card>
      )}

      {/* Card list / my packs */}
      <div style={{ flex: 1, overflow: 'auto' }} className="ol-thinscroll ol-scroll-fade">
        {loading && items.length === 0 ? (
          // Show loading only when there's no cached data; with a cache, render it directly and let the background refresh correct it
          <div
            style={{
              padding: 32,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              gap: 10,
              color: 'var(--ol-ink-4)',
              fontSize: 13,
            }}
          >
            <ThinkingDots size={18} />
            {t('common.loading')}
          </div>
        ) : visibleItems.length === 0 ? (
          <Card padding={28} style={{ textAlign: 'center' }}>
            <div style={{ fontSize: 13, color: 'var(--ol-ink-3)', marginBottom: 6 }}>
              {sort === 'liked' && t('marketplace.likedEmpty')}
              {(sort === 'popular' || sort === 'new') && t('marketplace.empty')}
            </div>
            <div style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>
              {sort === 'liked' && t('marketplace.likedEmptyHint')}
              {(sort === 'popular' || sort === 'new') && t('marketplace.emptyHint')}
            </div>
          </Card>
        ) : (
          <div
            className="ol-grid-auto-cards"
            style={{
              display: 'grid',
              gridTemplateColumns: stackLayout ? '1fr' : 'repeat(auto-fill, minmax(260px, 1fr))',
              gap: 12,
            }}
          >
            <AnimatePresence mode="sync">
              {visibleItems.map((p) => {
                const isDownloading = downloadingPackId === p.id;
                return (
                  <motion.article
                    layout
                    initial={{ opacity: 0, scale: 0.85 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, scale: 0.85 }}
                    transition={{
                      layout: { type: 'spring', damping: 25, stiffness: 220 },
                      opacity: { duration: 0.2 },
                      scale: { duration: 0.2 },
                    }}
                    key={p.id}
                    style={{
                      borderRadius: 12,
                      border: '0.5px solid var(--ol-line-strong)',
                      background: 'var(--ol-surface)',
                      display: 'flex',
                      flexDirection: 'column',
                      overflow: 'hidden',
                    }}
                  >
                    <button
                      type="button"
                      onClick={() => void openDetail(p.id)}
                      style={{
                        width: '100%',
                        flex: 1,
                        textAlign: 'left',
                        padding: 14,
                        border: 'none',
                        background: 'transparent',
                        cursor: 'pointer',
                        display: 'flex',
                        flexDirection: 'column',
                        gap: 6,
                      }}
                    >
                      <div
                        style={{
                          display: 'flex',
                          alignItems: 'baseline',
                          justifyContent: 'space-between',
                          gap: 6,
                        }}
                      >
                        <span style={{ fontSize: 14, fontWeight: 600, color: 'var(--ol-ink-1)' }}>
                          {p.name}
                        </span>
                        <span
                          style={{
                            fontSize: 10,
                            color: 'var(--ol-ink-4)',
                            fontFamily: 'var(--ol-font-mono)',
                          }}
                        >
                          v{p.version}
                        </span>
                      </div>
                      <div
                        style={{
                          fontSize: 12,
                          color: 'var(--ol-ink-3)',
                          lineHeight: 1.5,
                          display: '-webkit-box',
                          WebkitLineClamp: 2,
                          WebkitBoxOrient: 'vertical',
                          overflow: 'hidden',
                          minHeight: 36,
                        }}
                      >
                        {p.description || t('marketplace.noDescription')}
                      </div>
                      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 2 }}>
                        <Pill size="sm" tone="outline">
                          {p.baseMode}
                        </Pill>
                        {isDerivative(p.originAuthorLogin) && (
                          <span
                            title={t('marketplace.derivativeBadge', { login: p.originAuthorLogin })}
                          >
                            <Pill size="sm" tone="ok">
                              {t('marketplace.derivativeBadge', { login: p.originAuthorLogin })}
                            </Pill>
                          </span>
                        )}
                        {p.tags.slice(0, 2).map((tag) => (
                          <Pill key={tag} size="sm" tone="default">
                            {tag}
                          </Pill>
                        ))}
                      </div>
                    </button>
                    <div
                      style={{
                        display: 'flex',
                        justifyContent: 'space-between',
                        alignItems: 'center',
                        flexWrap: 'wrap',
                        gap: 8,
                        padding: '0 14px 12px',
                        fontSize: 11,
                        color: 'var(--ol-ink-4)',
                      }}
                    >
                      <span style={{ fontWeight: 500, color: 'var(--ol-ink-3)' }}>
                        @{p.authorLogin}
                      </span>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                        <span>
                          <span
                            style={{ color: likedIds.has(p.id) ? '#ef4444' : 'var(--ol-ink-4)' }}
                          >
                            {likedIds.has(p.id) ? '★' : '☆'}
                          </span>{' '}
                          {p.likeCount} · ↓ {p.downloadCount}
                        </span>
                        <button
                          type="button"
                          disabled={downloadingPackId !== null}
                          aria-label={t('marketplace.downloadAria', { name: p.name })}
                          title={t('marketplace.downloadAria', { name: p.name })}
                          onClick={() => void onDownload(p)}
                          style={{
                            display: 'inline-flex',
                            alignItems: 'center',
                            gap: 4,
                            padding: '4px 7px',
                            borderRadius: 7,
                            border: '0.5px solid var(--ol-line-strong)',
                            background: 'var(--ol-surface-2)',
                            color: 'var(--ol-ink-2)',
                            cursor: downloadingPackId === null ? 'pointer' : 'not-allowed',
                            opacity: downloadingPackId !== null && !isDownloading ? 0.5 : 1,
                            fontSize: 10,
                            fontWeight: 600,
                            whiteSpace: 'nowrap',
                          }}
                        >
                          <Icon name="download" size={12} />
                          {isDownloading
                            ? t('marketplace.downloadingZipBtn')
                            : t('marketplace.downloadZipBtn')}
                        </button>
                      </div>
                    </div>
                  </motion.article>
                );
              })}
            </AnimatePresence>
          </div>
        )}
      </div>

      {/* Detail dialog */}
      <PresenceModal
        open={Boolean(selectedId)}
        zIndex={mobile || stackLayout ? 70 : 50}
        onClose={() => {
          setSelectedId(null);
          setInstallError(null);
        }}
        render={() =>
          selectedId && (
            <>
              {detailLoading || !detail ? (
                <div
                  style={{
                    padding: 32,
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    gap: 10,
                    color: 'var(--ol-ink-4)',
                    fontSize: 13,
                  }}
                >
                  <ThinkingDots size={18} />
                  {t('common.loading')}
                </div>
              ) : (
                <>
                  <div
                    style={{
                      display: 'flex',
                      alignItems: 'baseline',
                      gap: 10,
                      marginBottom: 6,
                      flexWrap: 'wrap',
                    }}
                  >
                    <h2 style={{ margin: 0, fontSize: 18, fontWeight: 650 }}>{detail.name}</h2>
                    <Pill size="sm" tone="outline">
                      {detail.baseMode}
                    </Pill>
                    {isDerivative(detail.originAuthorLogin) && (
                      <span
                        title={t('marketplace.derivativeBadge', {
                          login: detail.originAuthorLogin,
                        })}
                      >
                        <Pill size="sm" tone="ok">
                          {t('marketplace.derivativeBadge', { login: detail.originAuthorLogin })}
                        </Pill>
                      </span>
                    )}
                    <span
                      style={{
                        fontSize: 11,
                        color: 'var(--ol-ink-4)',
                        fontFamily: 'var(--ol-font-mono)',
                      }}
                    >
                      v{detail.version}
                    </span>
                  </div>
                  <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginBottom: 12 }}>
                    <span style={{ fontWeight: 500, color: 'var(--ol-ink-3)' }}>
                      @{detail.authorLogin}
                    </span>
                    {' · '}
                    <span
                      style={{ color: likedIds.has(detail.id) ? '#ef4444' : 'var(--ol-ink-4)' }}
                    >
                      {likedIds.has(detail.id) ? '★' : '☆'}
                    </span>{' '}
                    {detail.likeCount}
                    {' · ↓ '}
                    {detail.downloadCount}
                  </div>
                  {detail.description && (
                    <div
                      style={{
                        fontSize: 13,
                        color: 'var(--ol-ink-2)',
                        lineHeight: 1.6,
                        marginBottom: 14,
                      }}
                    >
                      {detail.description}
                    </div>
                  )}
                  <div
                    style={{
                      padding: 12,
                      border: '0.5px solid var(--ol-line)',
                      borderRadius: 10,
                      background: 'var(--ol-surface-2)',
                      marginBottom: 14,
                      maxHeight: 280,
                      overflow: 'auto',
                      fontSize: 12,
                      fontFamily: 'var(--ol-font-mono)',
                      whiteSpace: 'pre-wrap',
                      color: 'var(--ol-ink-2)',
                    }}
                  >
                    {detail.prompt}
                  </div>
                  {isMarketplaceInstallErrorForPack(installError, detail.id) && (
                    <div
                      role="alert"
                      style={{
                        color: '#ef4444',
                        fontSize: 12,
                        whiteSpace: 'normal',
                        overflowWrap: 'anywhere',
                        marginBottom: 10,
                      }}
                    >
                      {installError.message}
                    </div>
                  )}
                  <div
                    style={{
                      display: 'flex',
                      justifyContent: 'space-between',
                      gap: 8,
                      alignItems: 'center',
                    }}
                  >
                    <div>
                      {marketplaceSignedIn &&
                        detail.authorLogin === currentLogin &&
                        currentLogin.length > 0 && (
                          <Btn variant="ghost" size="sm" onClick={() => void onDelete()}>
                            <span style={{ color: '#ef4444', marginRight: 4 }}>🗑</span>
                            {t('marketplace.detail.withdrawBtn')}
                          </Btn>
                        )}
                    </div>
                    <div style={{ display: 'flex', gap: 8 }}>
                      <motion.button
                        onClick={() => void onLike()}
                        aria-label={
                          marketplaceSignedIn ? undefined : t('marketplace.oauth.loginBtn')
                        }
                        style={{
                          display: 'inline-flex',
                          alignItems: 'center',
                          justifyContent: 'center',
                          background: 'transparent',
                          border: 'none',
                          cursor: 'pointer',
                          padding: '4px 8px',
                          borderRadius: 8,
                          fontSize: 12,
                          fontWeight: 500,
                          color: 'var(--ol-ink-2)',
                        }}
                      >
                        <span
                          style={{
                            color: likedIds.has(detail.id) ? '#ef4444' : 'inherit',
                            marginRight: 4,
                            display: 'inline-block',
                          }}
                        >
                          {likedIds.has(detail.id) ? '★' : '☆'}
                        </span>
                        {detail.likeCount}
                      </motion.button>
                      <Btn
                        variant="ghost"
                        size="sm"
                        onClick={() => {
                          setSelectedId(null);
                          setInstallError(null);
                        }}
                      >
                        {installingPackId !== null ? t('common.close') : t('common.cancel')}
                      </Btn>
                      <Btn
                        variant="blue"
                        size="sm"
                        disabled={!canStartMarketplaceInstall(installingPackId)}
                        onClick={() => void onInstall()}
                      >
                        {isMarketplaceInstallActive(installingPackId, detail.id)
                          ? t('marketplace.installingBtn')
                          : t('marketplace.installBtn')}
                      </Btn>
                    </div>
                  </div>
                </>
              )}
            </>
          )
        }
      />

      {/* Upload picker — zIndex 60 stacks it above "my packs" (zIndex 50) */}
      <PresenceModal
        open={Boolean(showUpload)}
        zIndex={mobile || stackLayout ? 70 : 60}
        onClose={() => {
          setShowUpload(false);
          setUploadOriginPackId(null);
          setUploadTargetName(null);
          setSelectedUploadPackId(null);
        }}
        render={() =>
          showUpload && (
            <>
              <h2 style={{ margin: '0 0 12px', fontSize: 16, fontWeight: 650 }}>
                {uploadOriginPackId
                  ? t('marketplace.upload.updateTitle', {
                      name: uploadTargetName ?? t('style.pack.title'),
                    })
                  : t('marketplace.uploadTitle')}
              </h2>
              <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', marginBottom: 12 }}>
                {uploadOriginPackId
                  ? t('marketplace.upload.updateHint')
                  : t('marketplace.uploadHint', { login: prefs?.marketplaceDevLogin ?? '' })}
              </div>
              <div
                style={{
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 8,
                  maxHeight: 360,
                  overflow: 'auto',
                }}
              >
                {localPacks.length === 0 ? (
                  <div
                    style={{
                      fontSize: 12,
                      color: 'var(--ol-ink-4)',
                      textAlign: 'center',
                      padding: 20,
                    }}
                  >
                    {t('marketplace.uploadNoLocal')}
                  </div>
                ) : (
                  localPacks.map((p) => {
                    const recommended =
                      !!uploadTargetName &&
                      p.name.trim().toLowerCase() === uploadTargetName.trim().toLowerCase();
                    const selected = selectedUploadPackId === p.id;
                    return (
                      <button
                        key={p.id}
                        type="button"
                        onClick={() =>
                          setSelectedUploadPackId((prev) => (prev === p.id ? null : p.id))
                        }
                        style={{
                          textAlign: 'left',
                          padding: 10,
                          border: selected
                            ? '1px solid var(--ol-blue)'
                            : '0.5px solid var(--ol-line-strong)',
                          borderRadius: 8,
                          background: selected ? 'var(--ol-blue-soft)' : 'var(--ol-surface)',
                          cursor: 'pointer',
                          display: 'flex',
                          alignItems: 'center',
                          gap: 10,
                        }}
                      >
                        {/* Selection circle: empty when unselected; solid blue + white check when selected */}
                        <span
                          style={{
                            flexShrink: 0,
                            width: 18,
                            height: 18,
                            borderRadius: 999,
                            border: selected
                              ? '1px solid var(--ol-blue)'
                              : '1px solid var(--ol-line-strong)',
                            background: selected ? 'var(--ol-blue)' : 'transparent',
                            display: 'inline-grid',
                            placeItems: 'center',
                            color: '#fff',
                            fontSize: 11,
                            fontWeight: 700,
                            transition: 'background 0.12s, border-color 0.12s',
                          }}
                        >
                          {selected && '✓'}
                        </span>
                        <div style={{ flex: 1, minWidth: 0 }}>
                          <div
                            style={{
                              display: 'flex',
                              alignItems: 'center',
                              gap: 8,
                              justifyContent: 'space-between',
                            }}
                          >
                            <div style={{ fontSize: 13, fontWeight: 600 }}>{p.name}</div>
                            {recommended && (
                              <Pill size="sm" tone="blue">
                                {t('marketplace.upload.recommendedBadge')}
                              </Pill>
                            )}
                          </div>
                          <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginTop: 2 }}>
                            {p.description || t('marketplace.noDescription')}
                          </div>
                        </div>
                      </button>
                    );
                  })
                )}
              </div>
              {/* Bottom: cancel / confirm upload (disabled when nothing selected) */}
              <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 14 }}>
                <Btn
                  variant="ghost"
                  size="sm"
                  onClick={() => {
                    setShowUpload(false);
                    setUploadOriginPackId(null);
                    setUploadTargetName(null);
                    setSelectedUploadPackId(null);
                  }}
                >
                  {t('common.cancel')}
                </Btn>
                <Btn
                  variant="blue"
                  size="sm"
                  disabled={!selectedUploadPackId}
                  onClick={() => {
                    if (selectedUploadPackId) void onUpload(selectedUploadPackId);
                  }}
                >
                  {t('marketplace.upload.confirmBtn')}
                </Btn>
              </div>
            </>
          )
        }
      />

      {/* My packs · dialog form (stacked above the marketplace page) */}
      <PresenceModal
        open={Boolean(showMyPacks)}
        zIndex={mobile || stackLayout ? 70 : 50}
        onClose={() => setShowMyPacks(false)}
        render={() =>
          showMyPacks && (
            <>
              {/* Top row: search (left) + username/login (center) + close × (right) */}
              <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 12 }}>
                {/* Search box (leftmost) */}
                <div
                  style={{
                    flex: 1,
                    display: 'flex',
                    alignItems: 'center',
                    gap: 6,
                    padding: '6px 10px',
                    border: '0.5px solid var(--ol-line-strong)',
                    borderRadius: 10,
                    background: 'var(--ol-surface)',
                  }}
                >
                  <Icon name="search" size={14} stroke="var(--ol-ink-3)" />
                  <input
                    type="search"
                    placeholder={t('marketplace.myPacks.searchPlaceholder')}
                    value={myPacksQuery}
                    onChange={(e) => setMyPacksQuery(e.target.value)}
                    autoFocus
                    style={{
                      flex: 1,
                      outline: 'none',
                      border: 0,
                      background: 'transparent',
                      fontSize: 13,
                      color: 'var(--ol-ink-1)',
                    }}
                  />
                </div>
                {/* Username + login chip. Click → GitHub OAuth Device Flow.
                Clicking again while signed in restarts the flow (account switch). */}
                <button
                  type="button"
                  title={
                    authorizedLogin
                      ? t('marketplace.oauth.reloginTooltip', { login: authorizedLogin })
                      : t('marketplace.oauth.loginTooltip')
                  }
                  onClick={() => setShowLogin(true)}
                  style={{
                    display: 'inline-flex',
                    alignItems: 'center',
                    gap: 6,
                    padding: '5px 10px',
                    borderRadius: 9,
                    border: '0.5px solid var(--ol-line-strong)',
                    background: authorizedLogin ? 'var(--ol-blue-soft)' : 'var(--ol-surface)',
                    color: authorizedLogin ? 'var(--ol-blue)' : 'var(--ol-ink-3)',
                    fontSize: 12,
                    fontWeight: 650,
                    cursor: 'pointer',
                    whiteSpace: 'nowrap',
                  }}
                >
                  <span
                    style={{
                      width: 18,
                      height: 18,
                      borderRadius: 999,
                      display: 'inline-grid',
                      placeItems: 'center',
                      background: authorizedLogin ? 'rgba(37,99,235,0.14)' : 'var(--ol-surface-2)',
                      fontSize: 10,
                      fontWeight: 750,
                    }}
                  >
                    {(authorizedLogin || '?').slice(0, 1).toUpperCase()}
                  </span>
                  <span>
                    {authorizedLogin ? `@${authorizedLogin}` : t('marketplace.oauth.loginBtn')}
                  </span>
                </button>
                {/* Close × */}
                <button
                  type="button"
                  aria-label={t('common.close')}
                  title={t('common.close')}
                  onClick={() => setShowMyPacks(false)}
                  style={{
                    width: 30,
                    height: 30,
                    borderRadius: 9,
                    display: 'inline-grid',
                    placeItems: 'center',
                    border: '0.5px solid var(--ol-line-strong)',
                    background: 'var(--ol-surface)',
                    color: 'var(--ol-ink-2)',
                    cursor: 'pointer',
                    fontSize: 18,
                    lineHeight: 1,
                    fontWeight: 500,
                  }}
                >
                  ×
                </button>
              </div>

              {/* Second row: count info (left) + refresh + upload (right). Counts use visibleMyPacks (withdrawn /
              superseded already removed) so they match the cards visible in the list. */}
              <div
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  gap: 8,
                  marginBottom: 12,
                }}
              >
                <div style={{ fontSize: 11.5, color: 'var(--ol-ink-3)' }}>
                  {(() => {
                    if (!marketplaceSignedIn) return t('marketplace.myPacks.notLoggedIn');
                    const activeCount = visibleMyPacks.length;
                    const pendingCount = visibleMyPacks.filter((p) => p.state === 'pending').length;
                    return pendingCount > 0
                      ? t('marketplace.myPacks.summaryPending', {
                          count: activeCount,
                          pending: pendingCount,
                        })
                      : t('marketplace.myPacks.summary', { count: activeCount });
                  })()}
                </div>
                <div style={{ display: 'flex', gap: 6 }}>
                  <Btn
                    icon="refresh"
                    variant="ghost"
                    size="sm"
                    onClick={() => void refreshMyPacks()}
                    disabled={!marketplaceSignedIn || myPacksLoading}
                  >
                    {t('common.refresh')}
                  </Btn>
                  <span title={canUpload ? '' : t('marketplace.uploadDisabledHint')}>
                    <Btn
                      icon="cloud"
                      variant="blue"
                      size="sm"
                      onClick={() => void openUploadPicker()}
                      disabled={!canUpload}
                    >
                      {t('marketplace.uploadBtn')}
                    </Btn>
                  </span>
                </div>
              </div>

              {/* Pack list. Four states: loading (first fetch/retry) → error (HTTP failure + inline retry)
              → empty (no packs / no match) → list. Loading has top priority so the user knows data is being fetched;
              error gets its own block with a retry button — more reliably reachable than a toast. */}
              {(() => {
                const hasLoadedAny = visibleMyPacks.length > 0 || myPacks.length > 0;
                if (myPacksLoading && !hasLoadedAny) {
                  return (
                    <div style={{ padding: '32px 12px', textAlign: 'center' }}>
                      <div style={{ fontSize: 13, color: 'var(--ol-ink-3)', marginBottom: 6 }}>
                        {t('marketplace.myPacks.loadingTitle')}
                      </div>
                      <div style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>
                        {t('marketplace.myPacks.loadingHint')}
                      </div>
                    </div>
                  );
                }
                if (myPacksError && !hasLoadedAny) {
                  return (
                    <div style={{ padding: '24px 12px', textAlign: 'center' }}>
                      <div
                        style={{ fontSize: 13, color: 'var(--ol-red, #ef4444)', marginBottom: 8 }}
                      >
                        {t('marketplace.myPacks.loadErrorTitle')}
                      </div>
                      <div
                        style={{
                          fontSize: 11.5,
                          color: 'var(--ol-ink-4)',
                          marginBottom: 12,
                          wordBreak: 'break-word',
                        }}
                      >
                        {myPacksError}
                      </div>
                      <Btn variant="blue" size="sm" onClick={() => void refreshMyPacks()}>
                        {t('marketplace.myPacks.loadErrorRetry')}
                      </Btn>
                    </div>
                  );
                }
                if (visibleMyPacks.length === 0) {
                  return (
                    <div style={{ padding: '32px 12px', textAlign: 'center' }}>
                      <div style={{ fontSize: 13, color: 'var(--ol-ink-3)', marginBottom: 6 }}>
                        {marketplaceSignedIn
                          ? myPacks.length === 0
                            ? t('marketplace.myPacks.emptyTitle')
                            : t('marketplace.myPacks.noMatch')
                          : t('marketplace.myPacks.notLoggedIn')}
                      </div>
                      {marketplaceSignedIn && myPacks.length === 0 && (
                        <div style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>
                          {t('marketplace.myPacks.emptyHint')}
                        </div>
                      )}
                    </div>
                  );
                }
                return null;
              })()}
              {visibleMyPacks.length > 0 && (
                <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
                  {visibleMyPacks.map((pack) => (
                    <div
                      key={pack.id}
                      style={{
                        padding: 14,
                        borderRadius: 12,
                        border: '0.5px solid var(--ol-line-strong)',
                        background: 'var(--ol-surface)',
                        display: 'flex',
                        flexDirection: 'column',
                        gap: 8,
                      }}
                    >
                      <div
                        style={{
                          display: 'flex',
                          justifyContent: 'space-between',
                          alignItems: 'flex-start',
                          gap: 8,
                        }}
                      >
                        <div style={{ minWidth: 0 }}>
                          <div
                            style={{
                              fontSize: 14,
                              fontWeight: 650,
                              color: 'var(--ol-ink)',
                              overflow: 'hidden',
                              textOverflow: 'ellipsis',
                              whiteSpace: 'nowrap',
                            }}
                          >
                            {pack.name}
                          </div>
                          <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginTop: 3 }}>
                            v{pack.version} · {new Date(pack.updatedAt).toLocaleDateString()}
                          </div>
                        </div>
                        <Pill
                          size="sm"
                          tone={pack.state === 'approved' ? 'ok' : 'outline'}
                          style={
                            pack.state === 'rejected' || pack.state === 'withdrawn'
                              ? { color: '#ef4444', borderColor: 'rgba(239,68,68,0.28)' }
                              : undefined
                          }
                        >
                          {statusLabel(pack.state, t)}
                        </Pill>
                      </div>
                      {pack.description && (
                        <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.5 }}>
                          {pack.description}
                        </div>
                      )}
                      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
                        <Pill size="sm" tone="outline">
                          {pack.baseMode}
                        </Pill>
                        {pack.tags.slice(0, 3).map((tag) => (
                          <Pill key={tag} size="sm" tone="default">
                            {tag}
                          </Pill>
                        ))}
                      </div>
                      <div
                        style={{
                          display: 'flex',
                          justifyContent: 'space-between',
                          alignItems: 'center',
                          gap: 8,
                          marginTop: 2,
                        }}
                      >
                        <span style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>
                          ★ {pack.likeCount} · ↓ {pack.downloadCount}
                        </span>
                        <div style={{ display: 'flex', gap: 6 }}>
                          <Btn
                            variant="ghost"
                            size="sm"
                            onClick={() => void openUploadPicker(pack.id, pack.name)}
                            disabled={!canUpload}
                          >
                            {t('marketplace.myPacks.actions.update')}
                          </Btn>
                          {pack.state !== 'withdrawn' && (
                            <Btn variant="ghost" size="sm" onClick={() => void onDeleteMine(pack)}>
                              <span style={{ color: '#ef4444' }}>
                                {t('marketplace.myPacks.actions.withdraw')}
                              </span>
                            </Btn>
                          )}
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </>
          )
        }
      />

      {/* GitHub login dialog */}
      {showLogin && (
        <GithubLoginModal onClose={() => setShowLogin(false)} onSuccess={onLoginSuccess} />
      )}
    </div>
  );
}

function statusLabel(state: string, t: (key: string) => string): string {
  switch (state) {
    case 'pending':
      return t('marketplace.state.pending');
    case 'approved':
      return t('marketplace.state.approved');
    case 'rejected':
      return t('marketplace.state.rejected');
    case 'withdrawn':
      return t('marketplace.state.withdrawn');
    case 'superseded':
      return t('marketplace.state.superseded');
    default:
      return state || t('marketplace.state.unknown');
  }
}

function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
