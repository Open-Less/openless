import {
  createContext,
  useContext,
  useMemo,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from 'react';
import { createPortal } from 'react-dom';
import { createModalStack } from '../overlays/modalStack';
import './viewport.css';

const legacyModalStack = createModalStack();
const unbounded = { boundary: null, portal: null, scale: true, modalStack: legacyModalStack };
const Viewport = createContext(
  unbounded as {
    boundary: HTMLElement | null;
    portal: HTMLElement | null;
    scale: boolean;
    modalStack: ReturnType<typeof createModalStack>;
  },
);

export const PopupContainer = createContext<HTMLElement | null>(null);

export function AppViewport({
  scale = true,
  className,
  children,
}: {
  scale?: boolean;
  className?: string;
  children: ReactNode;
}) {
  const [boundary, setBoundary] = useState<HTMLDivElement | null>(null);
  const [portal, setPortal] = useState<HTMLDivElement | null>(null);
  const [modalStack] = useState(createModalStack);
  const context = useMemo(
    () => ({ boundary, portal, scale, modalStack }),
    [boundary, portal, scale, modalStack],
  );

  return (
    <Viewport.Provider value={context}>
      <div
        ref={setBoundary}
        className={['ol-app-viewport', className].filter(Boolean).join(' ')}
        data-ol-scale={scale ? 'preferences' : 'fixed'}
        onClickCapture={(event) => modalStack.captureClick(event.target)}
        onKeyDownCapture={modalStack.captureKey}
      >
        <div className="ol-viewport-content" ref={modalStack.setContent} tabIndex={-1}>
          {boundary && portal && children}
        </div>
        <div className="ol-viewport-portals" ref={setPortal} />
      </div>
    </Viewport.Provider>
  );
}

export function useAppViewport() {
  return useContext(Viewport);
}

export function useModalActive() {
  const { modalStack } = useAppViewport();
  return useSyncExternalStore(modalStack.subscribe, modalStack.getActive, modalStack.getActive);
}

export function usePopupViewport() {
  const { boundary, portal } = useAppViewport();
  const container = useContext(PopupContainer) ?? portal;
  return { boundary: boundary ?? undefined, container: container ?? undefined };
}

export function ViewportPortal({ children }: { children: ReactNode }) {
  const { portal } = useAppViewport();
  return createPortal(children, portal ?? document.body);
}
