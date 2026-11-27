import { Menu } from '@base-ui/react/menu';
import { useEffect, useRef, type RefObject } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { usePopupViewport } from '../../ui/layout/AppViewport';

export function VocabularyWordMenu({
  anchor,
  focusTarget,
  label,
  open,
  onOpenChange,
  onRemove,
}: {
  anchor: RefObject<HTMLDivElement>;
  focusTarget: RefObject<HTMLButtonElement>;
  label: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation();
  const { boundary, container } = usePopupViewport();
  const removalRequested = useRef(false);
  useEffect(() => {
    if (!open) return;
    const scroll = anchor.current?.closest('.ol-vocab-scroll');
    const dismiss = () => onOpenChange(false);
    scroll?.addEventListener('scroll', dismiss, { passive: true });
    window.addEventListener('resize', dismiss);
    return () => {
      scroll?.removeEventListener('scroll', dismiss);
      window.removeEventListener('resize', dismiss);
    };
  }, [anchor, onOpenChange, open]);

  return (
    <Menu.Root
      open={open}
      onOpenChange={onOpenChange}
      modal={false}
      onOpenChangeComplete={(isOpen) => {
        if (isOpen || !removalRequested.current) return;
        removalRequested.current = false;
        // The confirmation restores focus to the card if the user cancels.
        focusTarget.current?.focus({ preventScroll: true });
        onRemove();
      }}
    >
      <Menu.Portal container={container} className="ol-popup-portal">
        <Menu.Positioner
          anchor={anchor}
          positionMethod="fixed"
          collisionBoundary={boundary}
          collisionPadding={8}
          side="bottom"
          align="start"
          sideOffset={6}
          className="ol-select-positioner"
        >
          <Menu.Popup
            aria-label={label}
            className="ol-select-popup ol-vocab-word-menu"
            finalFocus={focusTarget}
          >
            <Menu.Item
              className="ol-vocab-word-menu-delete"
              onClick={() => {
                removalRequested.current = true;
              }}
            >
              <Icon name="trash" size={14} />
              {t('common.delete')}
            </Menu.Item>
          </Menu.Popup>
        </Menu.Positioner>
      </Menu.Portal>
    </Menu.Root>
  );
}
