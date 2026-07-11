// Marketplace.tsx — Style Pack Marketplace 浏览面板。
//
// Phase A 目标（goal 1.a-e）：
//   (a) 后端验证 — 通过 marketplace_* IPC 跟后端通信
//   (b) 上传与拉取功能 — Install / Upload 按钮
//   (c) 单独弹窗界面 — modal-style detail 卡片
//   (d) 搜索框 — 顶部 input + server-side ?q=
//   (e) 按排名自动推荐 — 默认 sort=popular
//
// 后端 URL 走 prefs.marketplaceBaseUrl，dev 模式默认 http://127.0.0.1:8090；
// 用户在 Settings 填生产 URL 后客户端自动切换。
// dev 上传需要 prefs.marketplaceDevLogin（GitHub login 风格）—— 空时上传按钮 disabled。

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import { Icon } from '../components/Icon';
import { SavedToast } from '../components/SavedToast';
import { ThinkingDots } from '../components/ThinkingDots';
import { GithubLoginModal } from '../components/GithubLoginModal';
import { Modal } from '../components/ui/Modal';
import {
  fetchMarketplaceDetail,
  installMarketplacePack,
  likeMarketplacePack,
  listMarketplace,
  listStylePacks,
  marketplaceDelete,
  marketplaceMyLikes,
  marketplaceMyPacks,
  readMarketplaceDetailCache,
  readMarketplaceListCache,
  uploadMarketplacePack,
  writeMarketplaceDetailCache,
  writeMarketplaceListCache,
} from '../lib/ipc';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import type { MarketplaceDetail, MarketplaceListItem, MarketplaceMyPackItem, StylePack } from '../lib/types';
import { Btn, Card, PageHeader, Pill } from './_atoms';

type SortMode = 'popular' | 'new' | 'liked';

export function Marketplace() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();

  // 启动时尝试读缓存：上次默认视图（popular + 空 query）的列表，秒呈现。后台 refresh 校准。
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

  const [showUpload, setShowUpload] = useState(false);
  const [uploadOriginPackId, setUploadOriginPackId] = useState<string | null>(null);
  const [uploadTargetName, setUploadTargetName] = useState<string | null>(null);
  const [localPacks, setLocalPacks] = useState<StylePack[]>([]);
  // 上传选包器选中态：点 pack 卡片选中（不立刻上传），底部「确定上传」才真正提交。
  const [selectedUploadPackId, setSelectedUploadPackId] = useState<string | null>(null);
  const [myPacks, setMyPacks] = useState<MarketplaceMyPackItem[]>([]);
  // 「我的发布」改为弹框形态：showMyPacks 控制开关，myPacksQuery 是弹框内独立搜索词
  // （不与外层 marketplace 搜索 query 互相干扰）。
  const [showMyPacks, setShowMyPacks] = useState(false);
  const [myPacksQuery, setMyPacksQuery] = useState('');
  // 加载/错误三态：loading（首次拉取或重试时）、error（HTTP 失败 / 解析失败）、success（默认）。
  // 旧版只有 success 状态 + toast，导致：拉取中显示「你还没有发布过风格包」误导用户；
  // 失败后只弹 toast，没有 inline 重试入口。
  const [myPacksLoading, setMyPacksLoading] = useState(false);
  const [myPacksError, setMyPacksError] = useState<string | null>(null);
  // GitHub 登录弹窗开关 —— 登录流程交给共用的 <GithubLoginModal />。
  const [showLogin, setShowLogin] = useState(false);
  // 当前用户赞过的 pack id 集合 —— 用于红心渲染 + 「我赞过的」过滤。
  // 进入 marketplace 时拉一次；点星后本地 mutate。
  const [likedIds, setLikedIds] = useState<Set<string>>(new Set());
  const canUpload = (prefs?.marketplaceDevLogin ?? '').trim().length > 0;
  const currentLogin = (prefs?.marketplaceDevLogin ?? '').trim();
  // 「衍生自」只在 origin 作者 != 当前登录身份时显示 —— 自己的 pack 不要给自己挂衍生标签。
  const isDerivative = (originLogin: string | null | undefined): boolean =>
    !!originLogin && originLogin !== currentLogin;

  // search 防抖 300ms
  useEffect(() => {
    const id = window.setTimeout(() => setDebouncedQuery(query), 300);
    return () => window.clearTimeout(id);
  }, [query]);

  // 单调递增 seq 防 stale 响应覆盖：用户快速改 query / 切换 pack 时旧请求 response
  // 可能晚于新请求到达，比较 seq 丢弃过期结果。
  const reqSeqRef = useRef(0);
  const detailSeqRef = useRef(0);
  const refresh = useCallback(async () => {
    const seq = ++reqSeqRef.current;
    setLoading(true);
    setLoadError(null);
    try {
      // backend 只认 popular/new —— 'liked' 走 popular 拉一批回来，前端再过滤。
      const serverSort: 'popular' | 'new' =
        sort === 'liked' ? 'popular' : sort;
      const list = await listMarketplace({ query: debouncedQuery, sort: serverSort, limit: 50 });
      if (seq !== reqSeqRef.current) return; // stale response
      setItems(list);
      // 只缓存「默认视图」（popular + 空 query），重开时秒出。
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
    if (sort === 'liked') return items.filter(it => likedIds.has(it.id));
    return items;
  }, [items, sort, likedIds]);

  const visibleMyPacks = useMemo(() => {
    // 立刻隐藏 withdrawn / superseded：
    // - withdrawn：用户已主动下架，留 5 分钟窗口反而让计数对不上（用户原报告：发布 1 个、显示 2 个）。
    //   下架的反馈通过 actionMsg toast 给即可。
    // - superseded：新版上架后旧版的服务端 state，对用户来说该旧版本已经"被替换"，
    //   不应再算进「我的发布」当前在线列表。
    const q = myPacksQuery.trim().toLowerCase();
    return myPacks.filter(pack => {
      if (pack.state === 'withdrawn' || pack.state === 'superseded') return false;
      if (!q) return true;
      return pack.name.toLowerCase().includes(q)
        || pack.description.toLowerCase().includes(q)
        || pack.tags.some(tag => tag.toLowerCase().includes(q));
    });
  }, [myPacks, myPacksQuery]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // 拉一次「我赞过的」缓存，渲染红心 + 「我赞过的」过滤。登录身份变更时重拉。
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const ids = await marketplaceMyLikes();
        if (!cancelled) setLikedIds(new Set(ids));
      } catch (error) {
        console.warn('[marketplace] fetch my-likes failed', error);
      }
    })();
    return () => { cancelled = true; };
  }, [currentLogin]);

  const refreshMyPacks = useCallback(async () => {
    if (!currentLogin) {
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
      console.warn('[marketplace] fetch my-packs failed', error);
      const msg = errorMessage(error);
      setMyPacksError(msg);
      // 仍然弹 toast，行为兼容；inline error 让用户在弹框里能直接重试。
      setActionMsg({ kind: 'err', text: t('marketplace.myPacks.loadFailed', { err: msg }) });
    } finally {
      setMyPacksLoading(false);
    }
  }, [currentLogin, t]);

  useEffect(() => {
    void refreshMyPacks();
  }, [refreshMyPacks]);

  // 弹框打开时刷新一次「我的发布」，避免显示陈旧数据。
  useEffect(() => {
    if (showMyPacks && currentLogin) {
      void refreshMyPacks();
    }
  }, [showMyPacks, currentLogin, refreshMyPacks]);

  const openDetail = async (id: string) => {
    const seq = ++detailSeqRef.current;
    setSelectedId(id);
    setDetail(null);
    setDetailLoading(true);
    // 差量缓存命中：list 已经带 version+updatedAt，按三元组匹配本机 detail。
    // 命中 = 直接渲染、跳过网络；未命中 = 走 fetchMarketplaceDetail。
    const listItem = items.find(it => it.id === id);
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
      if (seq !== detailSeqRef.current) return; // stale: 用户已切到另一个 pack
      // 校验后回写：writeMarketplaceDetailCache 会做 ID / 大小校验。
      writeMarketplaceDetailCache(d);
      setDetail(d);
    } catch (error) {
      if (seq !== detailSeqRef.current) return;
      console.error('[marketplace] detail failed', error);
      setActionMsg({ kind: 'err', text: t('marketplace.errors.detail', { err: errorMessage(error) }) });
      setSelectedId(null);
    } finally {
      if (seq === detailSeqRef.current) setDetailLoading(false);
    }
  };

  const onInstall = async () => {
    if (!detail) return;
    try {
      await installMarketplacePack(detail.id);
      setActionMsg({ kind: 'ok', text: t('marketplace.installed', { name: detail.name }) });
      setSelectedId(null);
    } catch (error) {
      setActionMsg({ kind: 'err', text: t('marketplace.errors.install', { err: errorMessage(error) }) });
    }
  };

  const onLike = async () => {
    if (!detail) return;
    const packId = detail.id;
    const prevLikedIds = likedIds;
    const prevLikeCount = detail.likeCount;
    const wasLiked = prevLikedIds.has(packId);
    // optimistic mutate：立即切红心 + 调计数，让用户感觉点击即生效。
    const optimisticCount = Math.max(0, prevLikeCount + (wasLiked ? -1 : 1));
    setLikedIds(prev => {
      const next = new Set(prev);
      if (wasLiked) next.delete(packId);
      else next.add(packId);
      return next;
    });
    setDetail(prev => (prev && prev.id === packId ? { ...prev, likeCount: optimisticCount } : prev));
    setItems(prev => prev.map(p => (p.id === packId ? { ...p, likeCount: optimisticCount } : p)));
    try {
      const r = await likeMarketplacePack(packId);
      // 服务端回来后以服务端 likeCount / alreadyLiked 为准校准（防止并发或本地 drift）。
      setDetail(prev => (prev && prev.id === packId ? { ...prev, likeCount: r.likeCount } : prev));
      setItems(prev => prev.map(p => (p.id === packId ? { ...p, likeCount: r.likeCount } : p)));
      setLikedIds(prev => {
        const next = new Set(prev);
        if (r.alreadyLiked) next.add(packId);
        else next.delete(packId);
        return next;
      });
    } catch (error) {
      // rollback 到点击前的状态
      setLikedIds(prevLikedIds);
      setDetail(prev => (prev && prev.id === packId ? { ...prev, likeCount: prevLikeCount } : prev));
      setItems(prev => prev.map(p => (p.id === packId ? { ...p, likeCount: prevLikeCount } : p)));
      setActionMsg({ kind: 'err', text: t('marketplace.errors.like', { err: errorMessage(error) }) });
    }
  };

  const openUploadPicker = async (originPackId: string | null = null, targetName: string | null = null) => {
    try {
      setUploadOriginPackId(originPackId);
      setUploadTargetName(targetName);
      const packs = await listStylePacks();
      // 内置 pack 是只读模板，不能上传；更新时把同名本地版本排到最前面。
      const target = (targetName ?? '').trim().toLowerCase();
      const editable = packs
        .filter(p => p.kind !== 'builtin')
        .sort((a, b) => {
          const aMatch = target.length > 0 && a.name.trim().toLowerCase() === target;
          const bMatch = target.length > 0 && b.name.trim().toLowerCase() === target;
          if (aMatch !== bMatch) return aMatch ? -1 : 1;
          return a.name.localeCompare(b.name);
        });
      setLocalPacks(editable);
      // 更新流程下预选「建议更新」的本地包（同名），用户多数情况下一键确认。
      const recommended = target.length > 0
        ? editable.find(p => p.name.trim().toLowerCase() === target)
        : undefined;
      setSelectedUploadPackId(recommended?.id ?? null);
      setShowUpload(true);
    } catch (error) {
      setActionMsg({ kind: 'err', text: t('marketplace.errors.loadLocal', { err: errorMessage(error) }) });
    }
  };

  const onDelete = async () => {
    if (!detail) return;
    if (detail.authorLogin !== currentLogin) return; // 只有作者能删
    // eslint-disable-next-line no-alert
    if (!window.confirm(t('marketplace.detail.withdrawConfirm', { name: detail.name }))) return;
    try {
      await marketplaceDelete(detail.id);
      setActionMsg({ kind: 'ok', text: t('marketplace.detail.withdrawSuccess') });
      setSelectedId(null);
      // 撤回后立即从列表里去掉，再请求一次确认
      setItems(prev => prev.filter(p => p.id !== detail.id));
      void refresh();
    } catch (error) {
      setActionMsg({ kind: 'err', text: t('marketplace.detail.withdrawFailed', { err: errorMessage(error) }) });
    }
  };

  const onDeleteMine = async (pack: MarketplaceMyPackItem) => {
    if (pack.authorLogin !== currentLogin) return;
    // eslint-disable-next-line no-alert
    if (!window.confirm(t('marketplace.detail.withdrawConfirm', { name: pack.name }))) return;
    try {
      await marketplaceDelete(pack.id);
      setActionMsg({ kind: 'ok', text: t('marketplace.detail.withdrawSuccess') });
      setMyPacks(prev => prev.filter(p => p.id !== pack.id));
      setItems(prev => prev.filter(p => p.id !== pack.id));
      void refreshMyPacks();
    } catch (error) {
      setActionMsg({ kind: 'err', text: t('marketplace.detail.withdrawFailed', { err: errorMessage(error) }) });
    }
  };

  const onUpload = async (packId: string) => {
    const localPack = localPacks.find(p => p.id === packId);
    try {
      const result = await uploadMarketplacePack(packId, uploadOriginPackId);
      // optimistic：拿到 200 立即把这条包推到「我的发布」最前面，状态置为后端返回值（通常 'pending'）。
      // 避免等 1.5s / 5s 的 polling 才看到——后续 polling 会用服务端真实数据覆盖。
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
        setMyPacks(prev => {
          const idx = prev.findIndex(p => p.id === result.id);
          if (idx >= 0) {
            // 原作者更新同 id：保留 likes/downloads 等服务端计数，覆盖元信息 + 重置 state 为 pending。
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
      // issue #470：上传后给后端一点时间落库 + 跑审核，再用服务端真实数据校准一次
      // （审核状态可能 pending→approved/rejected）。乐观更新已即时反映「我的发布」，
      // 这里只需单次兜底刷新；取较长延时（5s）确保后端最终一致后能查到，去掉冗余的 1.5s 那次。
      window.setTimeout(() => { void refresh(); void refreshMyPacks(); }, 5000);
    } catch (error) {
      setActionMsg({ kind: 'err', text: t('marketplace.errors.upload', { err: errorMessage(error) }) });
    }
  };

  // GitHub 登录成功 → 写回 prefs.marketplaceDevLogin，让后续 X-Dev-User 走真实身份。
  const onLoginSuccess = useCallback((nextLogin: string) => {
    // prefs 写入失败只 console 记一笔（与重构前的 OAuth 轮询一致）—— 不能裸 void，
    // 否则 reject 会冒成未处理的 promise rejection。
    void updatePrefs(current => ({ ...current, marketplaceDevLogin: nextLogin }))
      .catch(e => console.warn('[marketplace] save login to prefs failed', e));
    setActionMsg({ kind: 'ok', text: t('marketplace.oauth.successAs', { login: nextLogin }) });
  }, [updatePrefs, t]);

  const sortPills = useMemo<Array<{ id: SortMode; label: string }>>(
    () => [
      { id: 'popular', label: t('marketplace.sortPopular') },
      { id: 'new', label: t('marketplace.sortNew') },
      { id: 'liked', label: t('marketplace.sortLiked') },
    ],
    [t],
  );

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', minHeight: 0, position: 'relative' }}>
      <PageHeader
        kicker={t('marketplace.kicker')}
        title={t('marketplace.title')}
        desc={t('marketplace.desc')}
        right={
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap', justifyContent: 'flex-end' }}>
            <button
              type="button"
              onClick={() => setShowMyPacks(true)}
              title={currentLogin ? t('marketplace.myPacks.buttonTitle', { login: currentLogin }) : t('marketplace.myPacks.buttonTitleEmpty')}
              style={{
                display: 'inline-flex', alignItems: 'center', gap: 8,
                height: 30, padding: '0 12px', borderRadius: 9,
                border: '0.5px solid var(--ol-line-strong)',
                background: 'var(--ol-surface)',
                color: 'var(--ol-ink-2)',
                fontSize: 12, fontWeight: 650,
                cursor: 'pointer',
                boxShadow: '0 1px 2px rgba(15,17,22,0.04)',
              }}
            >
              <span style={{
                width: 18, height: 18, borderRadius: 999,
                display: 'inline-grid', placeItems: 'center',
                background: 'var(--ol-surface-2)',
                fontSize: 10, fontWeight: 750,
              }}>
                {(currentLogin || '?').slice(0, 1).toUpperCase()}
              </span>
              <span>{t('marketplace.myPacks.buttonLabel')}</span>
            </button>
            <Btn icon="refresh" variant="ghost" size="sm" onClick={() => void refresh()}>
              {t('common.refresh')}
            </Btn>
          </div>
        }
      />

      {/* 顶部搜索 + 排序 */}
      <div
        style={{
          display: 'flex',
          gap: 10,
          alignItems: 'center',
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
            onChange={e => setQuery(e.target.value)}
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
        <div style={{ display: 'flex', gap: 4 }}>
          {sortPills.map(p => (
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
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 10 }}>
            <div style={{ fontSize: 12, color: 'var(--ol-err)', flex: 1, wordBreak: 'break-word' }}>
              {t('marketplace.loadFailed', { err: loadError })}
            </div>
            <Btn variant="blue" size="sm" onClick={() => void refresh()}>
              {t('common.retry') ?? '重试'}
            </Btn>
          </div>
        </Card>
      )}

      {/* 卡片列表 / 我的发布 */}
      <div style={{ flex: 1, overflow: 'auto' }} className="ol-thinscroll ol-scroll-fade">
        {loading && items.length === 0 ? (
          // 只在没有缓存数据时才显示 loading；有缓存就直接渲染缓存数据，后台 refresh 校准
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
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))', gap: 12 }}>
            <AnimatePresence mode="sync">
            {visibleItems.map(p => (
              <motion.button
                layout
                initial={{ opacity: 0, scale: 0.85 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.85 }}
                transition={{
                  layout: { type: 'spring', damping: 25, stiffness: 220 },
                  opacity: { duration: 0.2 },
                  scale: { duration: 0.2 }
                }}
                key={p.id}
                onClick={() => void openDetail(p.id)}
                style={{
                  textAlign: 'left',
                  padding: 14,
                  borderRadius: 12,
                  border: '0.5px solid var(--ol-line-strong)',
                  background: 'var(--ol-surface)',
                  cursor: 'pointer',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 6,
                }}
              >
                <div style={{ display: 'flex', alignItems: 'baseline', justifyContent: 'space-between', gap: 6 }}>
                  <span style={{ fontSize: 14, fontWeight: 600, color: 'var(--ol-ink-1)' }}>{p.name}</span>
                  <span style={{ fontSize: 10, color: 'var(--ol-ink-4)', fontFamily: 'var(--ol-font-mono)' }}>v{p.version}</span>
                </div>
                <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.5, display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical', overflow: 'hidden', minHeight: 36 }}>
                  {p.description || t('marketplace.noDescription')}
                </div>
                <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 2 }}>
                  <Pill size="sm" tone="outline">{p.baseMode}</Pill>
                  {isDerivative(p.originAuthorLogin) && (
                    <span title={t('marketplace.derivativeBadge', { login: p.originAuthorLogin })}>
                      <Pill size="sm" tone="ok">{t('marketplace.derivativeBadge', { login: p.originAuthorLogin })}</Pill>
                    </span>
                  )}
                  {p.tags.slice(0, 2).map(tag => <Pill key={tag} size="sm" tone="default">{tag}</Pill>)}
                </div>
                <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 11, color: 'var(--ol-ink-4)', marginTop: 4 }}>
                  <span style={{ fontWeight: 500, color: 'var(--ol-ink-3)' }}>@{p.authorLogin}</span>
                  <span>
                    <span style={{ color: likedIds.has(p.id) ? '#ef4444' : 'var(--ol-ink-4)' }}>{likedIds.has(p.id) ? '★' : '☆'}</span>
                    {' '}{p.likeCount} · ↓ {p.downloadCount}
                  </span>
                </div>
              </motion.button>
            ))}
            </AnimatePresence>
          </div>
        )}
      </div>

      {/* 详情弹窗 */}
      {selectedId && (
        <Modal onClose={() => setSelectedId(null)}>
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
              <div style={{ display: 'flex', alignItems: 'baseline', gap: 10, marginBottom: 6, flexWrap: 'wrap' }}>
                <h2 style={{ margin: 0, fontSize: 18, fontWeight: 650 }}>{detail.name}</h2>
                <Pill size="sm" tone="outline">{detail.baseMode}</Pill>
                {isDerivative(detail.originAuthorLogin) && (
                  <span title={t('marketplace.derivativeBadge', { login: detail.originAuthorLogin })}>
                    <Pill size="sm" tone="ok">{t('marketplace.derivativeBadge', { login: detail.originAuthorLogin })}</Pill>
                  </span>
                )}
                <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', fontFamily: 'var(--ol-font-mono)' }}>
                  v{detail.version}
                </span>
              </div>
              <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginBottom: 12 }}>
                <span style={{ fontWeight: 500, color: 'var(--ol-ink-3)' }}>@{detail.authorLogin}</span>
                {' · '}
                <span style={{ color: likedIds.has(detail.id) ? '#ef4444' : 'var(--ol-ink-4)' }}>
                  {likedIds.has(detail.id) ? '★' : '☆'}
                </span>
                {' '}{detail.likeCount}{' · ↓ '}{detail.downloadCount}
              </div>
              {detail.description && (
                <div style={{ fontSize: 13, color: 'var(--ol-ink-2)', lineHeight: 1.6, marginBottom: 14 }}>
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
              <div style={{ display: 'flex', justifyContent: 'space-between', gap: 8, alignItems: 'center' }}>
                <div>
                  {detail.authorLogin === currentLogin && currentLogin.length > 0 && (
                    <Btn variant="ghost" size="sm" onClick={() => void onDelete()}>
                      <span style={{ color: '#ef4444', marginRight: 4 }}>🗑</span>
                      {t('marketplace.detail.withdrawBtn')}
                    </Btn>
                  )}
                </div>
                <div style={{ display: 'flex', gap: 8 }}>
                  <motion.button
                    whileTap={{ scale: 0.75 }}
                    transition={{ type: 'spring', stiffness: 400, damping: 17 }}
                    onClick={() => void onLike()}
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
                      color: 'var(--ol-ink-2)'
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
                  <Btn variant="ghost" size="sm" onClick={() => setSelectedId(null)}>
                    {t('common.cancel')}
                  </Btn>
                  <Btn variant="blue" size="sm" onClick={() => void onInstall()}>
                    {t('marketplace.installBtn')}
                  </Btn>
                </div>
              </div>
            </>
          )}
        </Modal>
      )}

      {/* 上传选包器 —— zIndex 60 让它叠在「我的发布」(zIndex 50) 之上 */}
      {showUpload && (
        <Modal
          zIndex={60}
          onClose={() => {
            setShowUpload(false);
            setUploadOriginPackId(null);
            setUploadTargetName(null);
            setSelectedUploadPackId(null);
          }}
        >
          <h2 style={{ margin: '0 0 12px', fontSize: 16, fontWeight: 650 }}>
            {uploadOriginPackId ? t('marketplace.upload.updateTitle', { name: uploadTargetName ?? t('style.pack.title') }) : t('marketplace.uploadTitle')}
          </h2>
          <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', marginBottom: 12 }}>
            {uploadOriginPackId ? t('marketplace.upload.updateHint') : t('marketplace.uploadHint', { login: prefs?.marketplaceDevLogin ?? '' })}
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8, maxHeight: 360, overflow: 'auto' }}>
            {localPacks.length === 0 ? (
              <div style={{ fontSize: 12, color: 'var(--ol-ink-4)', textAlign: 'center', padding: 20 }}>
                {t('marketplace.uploadNoLocal')}
              </div>
            ) : (
              localPacks.map(p => {
                const recommended = !!uploadTargetName && p.name.trim().toLowerCase() === uploadTargetName.trim().toLowerCase();
                const selected = selectedUploadPackId === p.id;
                return (
                  <button
                    key={p.id}
                    type="button"
                    onClick={() => setSelectedUploadPackId(prev => (prev === p.id ? null : p.id))}
                    style={{
                      textAlign: 'left',
                      padding: 10,
                      border: selected ? '1px solid var(--ol-blue)' : '0.5px solid var(--ol-line-strong)',
                      borderRadius: 8,
                      background: selected ? 'var(--ol-blue-soft)' : 'var(--ol-surface)',
                      cursor: 'pointer',
                      display: 'flex',
                      alignItems: 'center',
                      gap: 10,
                    }}
                  >
                    {/* 选中圈：未选空圆，选中蓝实心 + 白勾 */}
                    <span style={{
                      flexShrink: 0,
                      width: 18, height: 18, borderRadius: 999,
                      border: selected ? '1px solid var(--ol-blue)' : '1px solid var(--ol-line-strong)',
                      background: selected ? 'var(--ol-blue)' : 'transparent',
                      display: 'inline-grid', placeItems: 'center',
                      color: '#fff', fontSize: 11, fontWeight: 700,
                      transition: 'background 0.12s, border-color 0.12s',
                    }}>
                      {selected && '✓'}
                    </span>
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8, justifyContent: 'space-between' }}>
                        <div style={{ fontSize: 13, fontWeight: 600 }}>{p.name}</div>
                        {recommended && <Pill size="sm" tone="blue">{t('marketplace.upload.recommendedBadge')}</Pill>}
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
          {/* 底部：取消 / 确定上传（未选中时 disabled）*/}
          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 14 }}>
            <Btn variant="ghost" size="sm" onClick={() => {
              setShowUpload(false);
              setUploadOriginPackId(null);
              setUploadTargetName(null);
              setSelectedUploadPackId(null);
            }}>
              {t('common.cancel')}
            </Btn>
            <Btn
              variant="blue"
              size="sm"
              disabled={!selectedUploadPackId}
              onClick={() => { if (selectedUploadPackId) void onUpload(selectedUploadPackId); }}
            >
              {t('marketplace.upload.confirmBtn')}
            </Btn>
          </div>
        </Modal>
      )}

      {/* 我的发布 · 弹框形态（叠在风格市场页面之上）*/}
      {showMyPacks && (
        <Modal onClose={() => setShowMyPacks(false)}>
          {/* 顶部一行：搜索 (左) + 用户名/登录 (中) + 关闭 × (右) */}
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 12 }}>
            {/* 搜索框 (最左) */}
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
                onChange={e => setMyPacksQuery(e.target.value)}
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
            {/* 用户名 + 登录 chip。点击 → 触发 GitHub OAuth Device Flow。
                已登录时再点会重新走一次（切账号）。 */}
            <button
              type="button"
              title={currentLogin ? t('marketplace.oauth.reloginTooltip', { login: currentLogin }) : t('marketplace.oauth.loginTooltip')}
              onClick={() => setShowLogin(true)}
              style={{
                display: 'inline-flex', alignItems: 'center', gap: 6,
                padding: '5px 10px', borderRadius: 9,
                border: '0.5px solid var(--ol-line-strong)',
                background: currentLogin ? 'var(--ol-blue-soft)' : 'var(--ol-surface)',
                color: currentLogin ? 'var(--ol-blue)' : 'var(--ol-ink-3)',
                fontSize: 12, fontWeight: 650,
                cursor: 'pointer',
                whiteSpace: 'nowrap',
              }}
            >
              <span style={{
                width: 18, height: 18, borderRadius: 999,
                display: 'inline-grid', placeItems: 'center',
                background: currentLogin ? 'rgba(37,99,235,0.14)' : 'var(--ol-surface-2)',
                fontSize: 10, fontWeight: 750,
              }}>
                {(currentLogin || '?').slice(0, 1).toUpperCase()}
              </span>
              <span>{currentLogin ? `@${currentLogin}` : t('marketplace.oauth.loginBtn')}</span>
            </button>
            {/* 关闭 × */}
            <button
              type="button"
              aria-label={t('common.close')}
              title={t('common.close')}
              onClick={() => setShowMyPacks(false)}
              style={{
                width: 30, height: 30, borderRadius: 9,
                display: 'inline-grid', placeItems: 'center',
                border: '0.5px solid var(--ol-line-strong)',
                background: 'var(--ol-surface)',
                color: 'var(--ol-ink-2)',
                cursor: 'pointer',
                fontSize: 18, lineHeight: 1,
                fontWeight: 500,
              }}
            >
              ×
            </button>
          </div>

          {/* 第二行：计数信息（左）+ 刷新 + 上传（右）。计数走 visibleMyPacks（已剔除
              withdrawn / superseded），跟列表里看到的卡片数对得上。 */}
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8, marginBottom: 12 }}>
            <div style={{ fontSize: 11.5, color: 'var(--ol-ink-3)' }}>
              {(() => {
                if (!currentLogin) return t('marketplace.myPacks.notLoggedIn');
                const activeCount = visibleMyPacks.length;
                const pendingCount = visibleMyPacks.filter(p => p.state === 'pending').length;
                return pendingCount > 0
                  ? t('marketplace.myPacks.summaryPending', { count: activeCount, pending: pendingCount })
                  : t('marketplace.myPacks.summary', { count: activeCount });
              })()}
            </div>
            <div style={{ display: 'flex', gap: 6 }}>
              <Btn icon="refresh" variant="ghost" size="sm" onClick={() => void refreshMyPacks()} disabled={!currentLogin || myPacksLoading}>
                {t('common.refresh')}
              </Btn>
              <span title={canUpload ? '' : t('marketplace.uploadDisabledHint')}>
                <Btn icon="cloud" variant="blue" size="sm" onClick={() => void openUploadPicker()} disabled={!canUpload}>
                  {t('marketplace.uploadBtn')}
                </Btn>
              </span>
            </div>
          </div>

          {/* 包列表。四态：loading（首次拉取/重试中）→ error（HTTP 失败 + inline 重试）
              → empty（无包/无匹配）→ list。loading 优先级最高，让用户清楚知道在拉数据；
              error 单独成块带「重试」按钮，比 toast 更稳定可达。 */}
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
                  <div style={{ fontSize: 13, color: 'var(--ol-red, #ef4444)', marginBottom: 8 }}>
                    {t('marketplace.myPacks.loadErrorTitle')}
                  </div>
                  <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginBottom: 12, wordBreak: 'break-word' }}>
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
                    {currentLogin
                      ? (myPacks.length === 0 ? t('marketplace.myPacks.emptyTitle') : t('marketplace.myPacks.noMatch'))
                      : t('marketplace.myPacks.notLoggedIn')}
                  </div>
                  {currentLogin && myPacks.length === 0 && (
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
              {visibleMyPacks.map(pack => (
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
                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', gap: 8 }}>
                    <div style={{ minWidth: 0 }}>
                      <div style={{ fontSize: 14, fontWeight: 650, color: 'var(--ol-ink)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{pack.name}</div>
                      <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginTop: 3 }}>v{pack.version} · {new Date(pack.updatedAt).toLocaleDateString()}</div>
                    </div>
                    <Pill size="sm" tone={pack.state === 'approved' ? 'ok' : 'outline'} style={pack.state === 'rejected' || pack.state === 'withdrawn' ? { color: '#ef4444', borderColor: 'rgba(239,68,68,0.28)' } : undefined}>{statusLabel(pack.state, t)}</Pill>
                  </div>
                  {pack.description && (
                    <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.5 }}>{pack.description}</div>
                  )}
                  <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
                    <Pill size="sm" tone="outline">{pack.baseMode}</Pill>
                    {pack.tags.slice(0, 3).map(tag => <Pill key={tag} size="sm" tone="default">{tag}</Pill>)}
                  </div>
                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 8, marginTop: 2 }}>
                    <span style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>★ {pack.likeCount} · ↓ {pack.downloadCount}</span>
                    <div style={{ display: 'flex', gap: 6 }}>
                      <Btn variant="ghost" size="sm" onClick={() => void openUploadPicker(pack.id, pack.name)} disabled={!canUpload}>
                        {t('marketplace.myPacks.actions.update')}
                      </Btn>
                      {pack.state !== 'withdrawn' && (
                        <Btn variant="ghost" size="sm" onClick={() => void onDeleteMine(pack)}>
                          <span style={{ color: '#ef4444' }}>{t('marketplace.myPacks.actions.withdraw')}</span>
                        </Btn>
                      )}
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </Modal>
      )}

      {/* GitHub 登录弹窗 */}
      {showLogin && (
        <GithubLoginModal
          onClose={() => setShowLogin(false)}
          onSuccess={onLoginSuccess}
        />
      )}
    </div>
  );
}

function statusLabel(state: string, t: (key: string) => string): string {
  switch (state) {
    case 'pending': return t('marketplace.state.pending');
    case 'approved': return t('marketplace.state.approved');
    case 'rejected': return t('marketplace.state.rejected');
    case 'withdrawn': return t('marketplace.state.withdrawn');
    case 'superseded': return t('marketplace.state.superseded');
    default: return state || t('marketplace.state.unknown');
  }
}

function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
