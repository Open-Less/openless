import type { ComponentPropsWithoutRef } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { IS_BETA_BUILD, IS_DEV_BUILD } from '../../lib/appVersion';
import type { OS } from '../../runtime/environment';
import { Tooltip } from '../../ui/overlays/Tooltip';
import { NAV_TREE, subItemLabelKey, type AppTab } from './navigation';

interface SidebarNavButtonProps extends Omit<ComponentPropsWithoutRef<'button'>, 'className'> {
  active?: boolean;
  layout?: 'item' | 'subitem' | 'footer-start' | 'footer-end';
}

function SidebarNavButton({
  active = false,
  layout = 'item',
  'aria-current': current,
  ...buttonProps
}: SidebarNavButtonProps) {
  const selected = active || current === 'page';
  return (
    <button
      {...buttonProps}
      type="button"
      aria-current={current}
      className={[
        'ol-nav-btn ol-sidebar-nav-btn',
        layout === 'subitem' && 'ol-nav-subitem',
        selected && 'ol-nav-btn-active',
      ]
        .filter(Boolean)
        .join(' ')}
      data-layout={layout}
    />
  );
}

interface DesktopSidebarProps {
  os: OS;
  currentTab: AppTab;
  settingsOpen: boolean;
  openGroups: Readonly<Record<string, boolean>>;
  onSelectTab: (tab: AppTab) => void;
  onPreloadTab: (tab: AppTab) => void;
  onToggleGroup: (key: string) => void;
  onOpenSettings: () => void;
}

export function DesktopSidebar({
  os,
  currentTab,
  settingsOpen,
  openGroups,
  onSelectTab,
  onPreloadTab,
  onToggleGroup,
  onOpenSettings,
}: DesktopSidebarProps) {
  const { t } = useTranslation();
  const visibleTab = settingsOpen ? undefined : currentTab;
  const pageButtonProps = (tab: AppTab) => ({
    onClick: () => onSelectTab(tab),
    onPointerEnter: () => onPreloadTab(tab),
    onFocus: () => onPreloadTab(tab),
    'aria-current': visibleTab === tab ? ('page' as const) : undefined,
  });

  return (
    <aside className="ol-sidebar-surface" data-os={os}>
      <button
        type="button"
        onClick={() => onSelectTab('overview')}
        className="ol-sidebar-brand"
        aria-label={t('shell.homeLabel')}
      >
        <strong>OpenLess</strong>
        {(IS_BETA_BUILD || IS_DEV_BUILD) && (
          <span className="ol-version-badge">{IS_DEV_BUILD ? 'DEV' : t('shell.betaTag')}</span>
        )}
      </button>

      <nav className="ol-sidebar-nav">
        {NAV_TREE.map((node) => {
          if (node.kind === 'item') {
            return (
              <Tooltip key={node.id} content={t(`shell.navHint.${node.id}`)} placement="right">
                <SidebarNavButton {...pageButtonProps(node.id)}>
                  <Icon name={node.icon} size={16} />
                  <span className="ol-sidebar-nav-label">{t(`nav.${node.id}`)}</span>
                </SidebarNavButton>
              </Tooltip>
            );
          }
          const expanded = !!openGroups[node.key];
          const containsCurrentPage = node.children.some((child) => child.id === visibleTab);
          return (
            <div key={node.key}>
              <SidebarNavButton
                onClick={() => onToggleGroup(node.key)}
                active={containsCurrentPage && !expanded}
                aria-expanded={expanded}
              >
                <Icon name={node.icon} size={16} />
                <span className="ol-sidebar-nav-label">{t(`nav.group.${node.key}`)}</span>
                <Icon name="chevRight" size={12} className="ol-sidebar-nav-chevron" />
              </SidebarNavButton>
              <div
                className="ol-sidebar-nav-group"
                aria-hidden={!expanded}
                ref={(element) => {
                  if (element) element.inert = !expanded;
                }}
              >
                <div className="ol-sidebar-nav-clip">
                  <div className="ol-sidebar-nav-children">
                    {node.children.map((child) => (
                      <SidebarNavButton
                        key={child.id}
                        {...pageButtonProps(child.id)}
                        layout="subitem"
                        tabIndex={expanded ? 0 : -1}
                      >
                        <span className="ol-sidebar-nav-label">{t(subItemLabelKey(child.id))}</span>
                      </SidebarNavButton>
                    ))}
                  </div>
                </div>
              </div>
            </div>
          );
        })}
      </nav>

      <div className="ol-sidebar-nav-footer">
        <Tooltip content={t('shell.navHint.settings')} placement="right">
          <SidebarNavButton
            layout="footer-start"
            onClick={() => onOpenSettings()}
            active={settingsOpen}
          >
            <Icon name="settings" size={16} />
            <span>{t('shell.footer.settings')}</span>
          </SidebarNavButton>
        </Tooltip>
        <SidebarNavButton
          layout="footer-end"
          onClick={() => onSelectTab('usage')}
          aria-current={visibleTab === 'usage' ? 'page' : undefined}
        >
          <Icon name="overview" size={16} />
          <span>{t('shell.footer.usage', { defaultValue: 'Usage' })}</span>
        </SidebarNavButton>
      </div>
    </aside>
  );
}
