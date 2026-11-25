import { useRef } from 'react';
import { Menu } from '@base-ui/react/menu';
import { usePopupViewport } from '../../ui/layout/AppViewport';
import './history.css';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';

interface HistoryActionMenuProps {
  hasAudioRecording: boolean;
  audioLoading: boolean;
  showShare: boolean;
  canRetranscribe: boolean;
  retranscribing: boolean;
  canRepolish: boolean;
  showSaveDirectory: boolean;
  exportDirectory: string;
  onPlay: () => void;
  onExport: () => void;
  onShare: () => void;
  onRetranscribe: () => void;
  onDelete: () => void | Promise<void>;
  onRepolish: () => void;
  onChooseSaveDirectory: () => void;
  onResetSaveDirectory: () => void;
}

export function HistoryActionMenu({
  hasAudioRecording,
  audioLoading,
  showShare,
  canRetranscribe,
  retranscribing,
  canRepolish,
  showSaveDirectory,
  exportDirectory,
  onPlay,
  onExport,
  onShare,
  onRetranscribe,
  onDelete,
  onRepolish,
  onChooseSaveDirectory,
  onResetSaveDirectory,
}: HistoryActionMenuProps) {
  const { t } = useTranslation();
  const { boundary, container } = usePopupViewport();
  const pendingAction = useRef<(() => void | Promise<void>) | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const run = (action: () => void | Promise<void>) => {
    pendingAction.current = action;
  };

  return (
    <Menu.Root
      modal={false}
      onOpenChangeComplete={(open) => {
        if (open || !pendingAction.current) return;
        const action = pendingAction.current;
        pendingAction.current = null;
        // A confirmation must restore focus to the menu trigger, not a removed menu item.
        trigger.current?.focus({ preventScroll: true });
        void action();
      }}
    >
      <Menu.Trigger
        ref={trigger}
        className="ol-pressable ol-action-button ol-history-menu-trigger"
        aria-label={t('history.actionMenu', 'Recording actions')}
        title={t('history.actionMenu', 'Recording actions')}
      >
        <Icon name="more" size={14} />
      </Menu.Trigger>
      <Menu.Portal container={container} className="ol-popup-portal">
        <Menu.Positioner
          positionMethod="fixed"
          collisionBoundary={boundary}
          collisionPadding={8}
          collisionAvoidance={{ side: 'flip', align: 'shift', fallbackAxisSide: 'none' }}
          side="bottom"
          align="end"
          sideOffset={6}
          className="ol-history-menu-positioner"
        >
          <Menu.Popup className="ol-history-menu">
            {hasAudioRecording && (
              <>
                <HistoryActionMenuItem
                  icon="play"
                  label={audioLoading ? t('history.audioLoading') : t('history.playRecording')}
                  disabled={audioLoading}
                  onClick={() => run(onPlay)}
                />
                <HistoryActionMenuItem
                  icon="download"
                  label={t('history.exportRecording')}
                  onClick={() => run(onExport)}
                />
                {showShare && (
                  <HistoryActionMenuItem
                    icon="upload"
                    label={t('quickNote.shareRecording', '分享录音')}
                    onClick={() => run(onShare)}
                  />
                )}
                <div style={{ height: 1, margin: '6px 4px', background: 'var(--ol-line-soft)' }} />
              </>
            )}
            {canRetranscribe && (
              <HistoryActionMenuItem
                icon="refresh"
                label={retranscribing ? t('history.retranscribing') : t('history.retranscribe')}
                disabled={retranscribing}
                onClick={() => run(onRetranscribe)}
              />
            )}
            <HistoryActionMenuItem
              icon="sparkle"
              label={t('history.repolish.title')}
              disabled={!canRepolish}
              onClick={() => run(onRepolish)}
            />
            <HistoryActionMenuItem
              icon="trash"
              label={t('common.delete')}
              danger
              onClick={() => run(onDelete)}
            />
            {showSaveDirectory && (
              <>
                <div style={{ height: 1, margin: '6px 4px', background: 'var(--ol-line-soft)' }} />
                <HistoryActionMenuItem
                  icon="archive"
                  label={
                    exportDirectory
                      ? t('history.changeSaveDirectory', '更改转写文件保存位置')
                      : t('history.saveDirectory', '设置转写文件保存位置')
                  }
                  detail={exportDirectory || t('history.defaultSaveDirectory', '每次导出时选择')}
                  onClick={() => run(onChooseSaveDirectory)}
                />
                {exportDirectory && (
                  <HistoryActionMenuItem
                    icon="x"
                    label={t('history.resetSaveDirectory', '恢复默认保存位置')}
                    onClick={() => run(onResetSaveDirectory)}
                  />
                )}
              </>
            )}
          </Menu.Popup>
        </Menu.Positioner>
      </Menu.Portal>
    </Menu.Root>
  );
}

function HistoryActionMenuItem({
  icon,
  label,
  detail,
  disabled = false,
  danger = false,
  onClick,
}: {
  icon: string;
  label: string;
  detail?: string;
  disabled?: boolean;
  danger?: boolean;
  onClick: () => void;
}) {
  return (
    <Menu.Item
      className="ol-history-menu-item"
      label={label}
      disabled={disabled}
      onClick={onClick}
      style={{
        width: '100%',
        display: 'flex',
        alignItems: 'center',
        gap: 9,
        padding: '8px 9px',
        border: 0,
        borderRadius: 'var(--ol-control-radius)',
        color: danger ? 'var(--ol-red, #ef4444)' : 'var(--ol-ink-2)',
        fontFamily: 'inherit',
        fontSize: 12.5,
        textAlign: 'left',
        cursor: disabled ? 'not-allowed' : 'pointer',
        opacity: disabled ? 0.5 : 1,
      }}
    >
      <Icon name={icon} size={14} />
      <span style={{ minWidth: 0, flex: 1 }}>
        <span style={{ display: 'block' }}>{label}</span>
        {detail && (
          <span
            title={detail}
            style={{
              display: 'block',
              marginTop: 2,
              overflow: 'hidden',
              color: 'var(--ol-ink-4)',
              fontSize: 10.5,
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            {detail}
          </span>
        )}
      </span>
    </Menu.Item>
  );
}
