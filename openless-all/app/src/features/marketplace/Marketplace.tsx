import { AnimatePresence, motion } from 'framer-motion';
import { ExpandingSearch } from '../../components/ExpandingSearch';
import { GithubLoginModal } from '../../components/GithubLoginModal';
import { Icon } from '../../components/Icon';
import { LikeButton } from './LikeButton';
import { ThinkingDots } from '../../components/ThinkingDots';
import { ExitPresence, PresenceModal } from '../../components/ui/Modal';
import {
  canStartMarketplaceInstall,
  isMarketplaceInstallActive,
  isMarketplaceInstallErrorForPack,
} from '../../lib/marketplaceInstall';
import { Button as Btn } from '../../ui/controls/Button';
import { Card } from '../../ui/layout/Card';
import { PageHeader } from '../../ui/layout/PageHeader';
import { Pill } from '../../ui/controls/Pill';
import { useMarketplaceWorkspace } from './useMarketplaceWorkspace';

export function Marketplace() {
  const {
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
  } = useMarketplaceWorkspace();
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
                borderRadius: 'var(--ol-r-md)',
                border: '0.5px solid var(--ol-line-strong)',
                background: 'var(--ol-surface)',
                color: 'var(--ol-ink-2)',
                fontSize: 12,
                fontWeight: 'var(--ol-font-weight-demibold)',
                cursor: 'pointer',
                boxShadow: '0 1px 2px rgba(15,17,22,0.04)',
              }}
            >
              <span
                style={{
                  width: 18,
                  height: 18,
                  borderRadius: 'var(--ol-radius)',
                  display: 'inline-grid',
                  placeItems: 'center',
                  background: 'var(--ol-surface-2)',
                  fontSize: 10,
                  fontWeight: 'calc(750 + var(--ol-font-weight-offset))',
                }}
              >
                {(authorizedLogin || '?').slice(0, 1).toUpperCase()}
              </span>
              <span>{t('marketplace.myPacks.buttonLabel')}</span>
            </button>
          </div>
        }
      />

      {/* Sort stays put; search is the dictionary's circular control, expanding left. */}
      <div
        className="ol-flex-row"
        style={{
          display: 'flex',
          gap: 10,
          alignItems: stackLayout ? 'stretch' : 'center',
          flexDirection: stackLayout ? 'column' : 'row',
          justifyContent: 'space-between',
          padding: '4px 0 14px',
        }}
      >
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
                borderRadius: 'var(--ol-control-radius)',
                cursor: 'pointer',
                background: sort === p.id ? 'var(--ol-blue-soft)' : 'var(--ol-surface)',
                color: sort === p.id ? 'var(--ol-blue)' : 'var(--ol-ink-2)',
              }}
            >
              {p.label}
            </button>
          ))}
        </div>
        <ExpandingSearch
          value={query}
          onChange={setQuery}
          placeholder={t('marketplace.searchPlaceholder')}
          label={t('marketplace.searchPlaceholder')}
        />
      </div>

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
        {(loading && items.length === 0) || (sort === 'liked' && !likesSettled) ? (
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
            className="ol-grid-auto-cards ol-grid-12 ol-grid-cards"
            data-columns={stackLayout ? '1' : undefined}
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
                      layout: { type: 'spring', damping: 32, stiffness: 220 },
                      opacity: { duration: 0.2 },
                      scale: { duration: 0.2 },
                    }}
                    key={p.id}
                    style={{
                      borderRadius: 'var(--ol-r-lg)',
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
                        <span
                          style={{
                            fontSize: 14,
                            fontWeight: 'var(--ol-font-weight-semibold)',
                            color: 'var(--ol-ink-1)',
                          }}
                        >
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
                      <span
                        style={{
                          fontWeight: 'var(--ol-font-weight-medium)',
                          color: 'var(--ol-ink-3)',
                        }}
                      >
                        @{p.authorLogin}
                      </span>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                        <span style={{ display: 'inline-flex', alignItems: 'center', gap: 8 }}>
                          <LikeButton
                            compact
                            liked={likedIds.has(p.id)}
                            count={p.likeCount}
                            label={t('marketplace.likeBtn')}
                            onClick={() => void onLike(p.id, p.likeCount)}
                          />
                          <span>↓ {p.downloadCount}</span>
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
                            borderRadius: 'var(--ol-control-radius)',
                            border: '0.5px solid var(--ol-line-strong)',
                            background: 'var(--ol-surface-2)',
                            color: 'var(--ol-ink-2)',
                            cursor: downloadingPackId === null ? 'pointer' : 'not-allowed',
                            opacity: downloadingPackId !== null && !isDownloading ? 0.5 : 1,
                            fontSize: 10,
                            fontWeight: 'var(--ol-font-weight-semibold)',
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
        onClose={closeDetail}
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
                    <h2
                      style={{
                        margin: 0,
                        fontSize: 18,
                        fontWeight: 'var(--ol-font-weight-demibold)',
                      }}
                    >
                      {detail.name}
                    </h2>
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
                    <span
                      style={{
                        fontWeight: 'var(--ol-font-weight-medium)',
                        color: 'var(--ol-ink-3)',
                      }}
                    >
                      @{detail.authorLogin}
                    </span>
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
                      borderRadius: 'var(--ol-r-md)',
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
                      <LikeButton
                        liked={likedIds.has(detail.id)}
                        count={detail.likeCount}
                        label={
                          marketplaceSignedIn
                            ? t('marketplace.likeBtn')
                            : t('marketplace.oauth.loginBtn')
                        }
                        onClick={() => void onLike(detail.id, detail.likeCount)}
                      />
                      <Btn variant="ghost" size="sm" onClick={closeDetail}>
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
              <h2
                style={{
                  margin: '0 0 12px',
                  fontSize: 16,
                  fontWeight: 'var(--ol-font-weight-demibold)',
                }}
              >
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
                          borderRadius: 'var(--ol-control-radius)',
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
                            borderRadius: 'var(--ol-radius)',
                            border: selected
                              ? '1px solid var(--ol-blue)'
                              : '1px solid var(--ol-line-strong)',
                            background: selected ? 'var(--ol-blue)' : 'transparent',
                            display: 'inline-grid',
                            placeItems: 'center',
                            color: '#fff',
                            fontSize: 11,
                            fontWeight: 'var(--ol-font-weight-bold)',
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
                            <div
                              style={{ fontSize: 13, fontWeight: 'var(--ol-font-weight-semibold)' }}
                            >
                              {p.name}
                            </div>
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
                    borderRadius: 'var(--ol-r-md)',
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
                    borderRadius: 'var(--ol-r-md)',
                    border: '0.5px solid var(--ol-line-strong)',
                    background: authorizedLogin ? 'var(--ol-blue-soft)' : 'var(--ol-surface)',
                    color: authorizedLogin ? 'var(--ol-blue)' : 'var(--ol-ink-3)',
                    fontSize: 12,
                    fontWeight: 'var(--ol-font-weight-demibold)',
                    cursor: 'pointer',
                    whiteSpace: 'nowrap',
                  }}
                >
                  <span
                    style={{
                      width: 18,
                      height: 18,
                      borderRadius: 'var(--ol-radius)',
                      display: 'inline-grid',
                      placeItems: 'center',
                      background: authorizedLogin ? 'rgba(37,99,235,0.14)' : 'var(--ol-surface-2)',
                      fontSize: 10,
                      fontWeight: 'calc(750 + var(--ol-font-weight-offset))',
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
                    borderRadius: 'var(--ol-r-md)',
                    display: 'inline-grid',
                    placeItems: 'center',
                    border: '0.5px solid var(--ol-line-strong)',
                    background: 'var(--ol-surface)',
                    color: 'var(--ol-ink-2)',
                    cursor: 'pointer',
                    fontSize: 18,
                    lineHeight: 1,
                    fontWeight: 'var(--ol-font-weight-medium)',
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
                        borderRadius: 'var(--ol-r-lg)',
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
                              fontWeight: 'var(--ol-font-weight-demibold)',
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
      <ExitPresence
        open={showLogin}
        render={(closing) => (
          <GithubLoginModal
            closing={closing}
            onClose={() => setShowLogin(false)}
            onSuccess={onLoginSuccess}
          />
        )}
      />
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
