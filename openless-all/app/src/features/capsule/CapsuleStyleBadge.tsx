import { Popover } from '@base-ui/react/popover';
import { memo, useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type {
  CapsuleStyleMenuKey,
  CapsuleStyleMenuRequest,
  DictationStyleTarget,
} from '../../api/contracts';
import {
  createCapsuleSubscriptions,
  getCapsuleStyleMenuRequest,
  getDictationStyleTarget,
  setCapsuleStyleMenu,
  updateDictationStyle,
} from '../../api/dictation';
import { Icon } from '../../components/Icon';
import { stylePackDisplayName } from '../../lib/stylePackPresentation';
import { getRuntimeEnvironment } from '../../runtime/environment';
import { usePopupViewport } from '../../ui/layout/AppViewport';
import {
  capsuleOutputStyleLabel,
  CapsuleStylePickerRequests,
  forgetStylePickerRequest,
  styleMenuKeyAction,
  stylePickerRequestForSession,
  type CapsuleOutputStyle,
} from './capsuleOutputStyle';
import type { CapsulePickerDeps, CapsulePickerMenu } from './capsulePickerMenu';
import { useCapsulePickerMenu } from './useCapsulePickerMenu';
import '../../ui/styles/select.css';
import './CapsuleStyles.css';

export const CapsuleStyleBadge = memo(function CapsuleStyleBadge({
  style,
  pickerController,
  pickerEnabled,
  captureSessionId,
}: {
  style: CapsuleOutputStyle | null;
  pickerController: CapsulePickerMenu;
  pickerEnabled: boolean;
  captureSessionId?: string;
}) {
  const { t } = useTranslation();
  const { boundary, container } = usePopupViewport();
  const anchor = useRef<HTMLDivElement>(null);
  const highlightedOption = useRef<HTMLButtonElement>(null);
  const onKey = useRef<(event: Pick<CapsuleStyleMenuKey, 'key' | 'repeat'>) => boolean>(
    () => false,
  );
  const failureId = useId();
  const [windowLabel, setWindowLabel] = useState<string>();
  const [request, setRequest] = useState<CapsuleStyleMenuRequest | null>(null);
  const [requests] = useState(() => new CapsuleStylePickerRequests());
  const [loaded, setLoaded] = useState<
    (CapsuleStyleMenuRequest & { target: DictationStyleTarget }) | null
  >(null);
  const [error, setError] = useState(false);
  const [saving, setSaving] = useState(false);
  const choice = useRef<symbol>();
  const [selectedId, setSelectedId] = useState('');
  const [highlightedId, setHighlightedId] = useState('');
  const cursor = useRef('');

  useEffect(() => {
    setRequest(null);
    setWindowLabel(undefined);
    if (!pickerEnabled || !captureSessionId || getRuntimeEnvironment().isMobile) return;
    const events = createCapsuleSubscriptions();
    let disposed = false;
    let requestRevision = 0;
    void (async () => {
      const label = await events.label();
      if (disposed || label !== 'capsule') return;
      if (!(await events.styleMenuKeys((event) => pickerController.receiveStyleKey(event)))) return;
      if (!(await events.styleMenuClosed((event) => pickerController.receiveStyleClose(event))))
        return;
      if (
        !(await events.styleMenuRequests((next) => {
          if (next.sessionId !== captureSessionId || requests.hasConsumed(next)) return;
          requestRevision += 1;
          setRequest(next);
        }))
      )
        return;
      setWindowLabel(label);
      const revisionAtRead = requestRevision;
      const pending = await getCapsuleStyleMenuRequest();
      if (
        !disposed &&
        requestRevision === revisionAtRead &&
        pending &&
        pending.sessionId === captureSessionId &&
        !requests.hasConsumed(pending)
      ) {
        setRequest(pending);
      }
    })().catch((error) => {
      if (!disposed) console.warn('[capsule] style picker input subscription failed', error);
    });
    return () => {
      disposed = true;
      events.dispose();
    };
  }, [captureSessionId, pickerController, pickerEnabled, requests]);

  const intent = stylePickerRequestForSession(
    request,
    captureSessionId,
    pickerEnabled && Boolean(windowLabel) && Boolean(style),
  );
  const requestId = intent?.requestId;
  const sessionId = intent?.sessionId;
  const target =
    requestId && loaded?.requestId === requestId && loaded.sessionId === sessionId
      ? loaded.target
      : null;
  const setNativeMenu = useCallback<CapsulePickerDeps['setNativeMenu']>(
    async (open, { sessionId, requestId }, nativeOwner) => {
      if (!open) {
        if (nativeOwner)
          await setCapsuleStyleMenu(false, sessionId, { menuId: nativeOwner.menuId });
        return;
      }
      const owner = await setCapsuleStyleMenu(true, sessionId, { requestId });
      if (!owner?.menuId || owner.sessionId !== sessionId || owner.windowLabel !== windowLabel) {
        throw new Error('The host did not return a matching style menu owner');
      }
      return owner;
    },
    [windowLabel],
  );
  const menu = useCapsulePickerMenu({
    controller: pickerController,
    kind: 'style',
    anchor,
    target,
    visible: Boolean(intent),
    setNativeMenu,
    onFailure: () => setError(true),
    onStaleStyleRequest: (stale) => {
      setRequest((current) => forgetStylePickerRequest(current, stale));
      setLoaded((current) => forgetStylePickerRequest(current, stale));
    },
  });

  useEffect(() => {
    setError(false);
    setSaving(false);
    setLoaded(null);
    choice.current = undefined;
    if (!requestId || !sessionId || requests.hasConsumed({ requestId, sessionId })) return;
    let disposed = false;
    let retry: ReturnType<typeof setTimeout> | undefined;
    const loadTarget = async () => {
      try {
        const next = await getDictationStyleTarget();
        if (disposed) return;
        if (!next || (next.sessionId === sessionId && !next.mutable)) {
          // The host can announce a tap during Starting, before the style context freezes.
          retry = setTimeout(() => void loadTarget(), 120);
          return;
        }
        if (next.sessionId === sessionId && next.selection.mode === 'skipAuto') {
          setLoaded({ sessionId, requestId, target: next });
        }
      } catch {
        if (!disposed) setError(true);
      }
    };
    void loadTarget();
    return () => {
      disposed = true;
      clearTimeout(retry);
      choice.current = undefined;
    };
  }, [requestId, requests, sessionId]);

  const highlight = (stylePackId: string) => {
    cursor.current = stylePackId;
    setHighlightedId(stylePackId);
  };

  useEffect(() => {
    if (!intent || !target) return;
    const initial = requests.take(intent, target);
    if (!initial) return;
    setSelectedId(initial);
    highlight(initial);
    void menu.openMenu(intent.requestId);
  }, [intent, menu.openMenu, requests, target]);

  useLayoutEffect(() => {
    if (menu.open)
      highlightedOption.current?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }, [menu.open, highlightedId]);

  useLayoutEffect(() => {
    if (menu.open && sessionId && requestId) {
      pickerController.enableStyleKeys(sessionId, requestId, (event) => onKey.current(event));
    }
  }, [menu.open, pickerController, requestId, sessionId]);

  const choose = async (stylePackId: string) => {
    if (
      !intent ||
      !menu.open ||
      !menu.owns(intent.sessionId, intent.requestId) ||
      !target?.availableStyles.some((pack) => pack.id === stylePackId) ||
      choice.current
    )
      return;
    const inFlight = Symbol();
    choice.current = inFlight;
    setSaving(true);
    setError(false);
    try {
      const updated = await updateDictationStyle(intent.sessionId, {
        mode: 'stylePack',
        stylePackId,
      });
      if (
        choice.current === inFlight &&
        updated.sessionId === intent.sessionId &&
        menu.owns(intent.sessionId, intent.requestId)
      )
        menu.close();
    } catch {
      if (choice.current === inFlight && menu.owns(intent.sessionId, intent.requestId)) {
        setError(true);
      }
    } finally {
      if (choice.current === inFlight) {
        choice.current = undefined;
        setSaving(false);
      }
    }
  };

  onKey.current = (event) => {
    if (!intent || !menu.owns(intent.sessionId, intent.requestId)) return false;
    const action = styleMenuKeyAction(event, target?.availableStyles ?? [], cursor.current);
    if (!action) return false;
    if (action.type === 'close') menu.close();
    else if (action.type === 'highlight') highlight(action.stylePackId);
    else void choose(action.stylePackId);
    return true;
  };

  useEffect(() => {
    if (!intent) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.isComposing || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey)
        return;
      const { key } = event;
      if (key !== 'ArrowUp' && key !== 'ArrowDown' && key !== 'Enter' && key !== 'Escape') return;
      if (onKey.current({ key, repeat: event.repeat })) {
        event.preventDefault();
        event.stopPropagation();
      }
    };
    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [intent]);

  if (!style) return null;
  const modeLabels = {
    raw: t('style.modes.raw.name'),
    light: t('style.modes.light.name'),
    structured: t('style.modes.structured.name'),
    formal: t('style.modes.formal.name'),
  };
  const failed = error && !menu.open;
  const label = failed
    ? t('capsule.styleChangeFailed')
    : (capsuleOutputStyleLabel(style, modeLabels) ?? t('settings.shortcuts.skipAutoStyle'));

  return (
    <>
      <div ref={anchor} className="ol-capsule-style-badge" data-error={failed || undefined}>
        <Icon name="sparkle" size={12} />
        <span
          role={failed ? 'alert' : 'status'}
          aria-label={t('capsule.styleStatus')}
          title={label}
        >
          {label}
        </span>
      </div>
      <Popover.Root
        open={menu.open}
        onOpenChange={(next) => {
          if (!next) menu.close();
        }}
        modal={false}
      >
        <Popover.Portal container={container} className="ol-popup-portal">
          <Popover.Positioner
            anchor={anchor}
            className="ol-select-positioner"
            positionMethod="fixed"
            collisionBoundary={boundary}
            side="top"
            align="center"
            sideOffset={8}
            collisionPadding={8}
            collisionAvoidance={{ side: 'flip', align: 'shift', fallbackAxisSide: 'none' }}
          >
            <Popover.Popup
              className="ol-select-popup ol-capsule-picker-popup ol-capsule-style-popup"
              aria-label={t('capsule.styleChoose')}
              initialFocus={false}
              finalFocus={false}
            >
              <div
                className="ol-select-list"
                role="listbox"
                aria-label={t('capsule.styleChoose')}
                aria-busy={saving}
                aria-describedby={error ? failureId : undefined}
              >
                {target?.availableStyles.map((pack) => {
                  const selected = pack.id === selectedId;
                  const highlighted = pack.id === highlightedId;
                  return (
                    <button
                      type="button"
                      role="option"
                      tabIndex={-1}
                      key={pack.id}
                      ref={highlighted ? highlightedOption : undefined}
                      className="ol-select-item ol-capsule-style-option"
                      aria-selected={selected}
                      data-selected={selected || undefined}
                      data-highlighted={highlighted || undefined}
                      onPointerMove={() => highlight(pack.id)}
                      onMouseDown={(event) => event.preventDefault()}
                      onClick={() => void choose(pack.id)}
                    >
                      <span>{stylePackDisplayName(pack, modeLabels)}</span>
                      {selected && <Icon name="check" size={12} className="ol-select-check" />}
                    </button>
                  );
                })}
              </div>
              {error && (
                <span id={failureId} className="ol-capsule-style-error" role="alert">
                  {t('capsule.styleChangeFailed')}
                </span>
              )}
            </Popover.Popup>
          </Popover.Positioner>
        </Popover.Portal>
      </Popover.Root>
    </>
  );
});
