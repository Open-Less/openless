import {
  createContext,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import { createInteractionController } from './interactionController';
import { ConfirmDialogHost } from './ConfirmDialogHost';
import { NotificationCenter } from '../feedback/NotificationCenter';
import './interaction.css';

const Controller = createContext<ReturnType<typeof createInteractionController> | null>(null);

export function InteractionProvider({ children }: { children: ReactNode }) {
  const [controller] = useState(createInteractionController);
  useEffect(() => {
    window.addEventListener('pagehide', controller.clear);
    return () => {
      window.removeEventListener('pagehide', controller.clear);
      controller.clear();
    };
  }, [controller]);
  return (
    <Controller.Provider value={controller}>
      {children}
      <ConfirmDialogHost controller={controller} />
      <NotificationCenter controller={controller} />
    </Controller.Provider>
  );
}

export function useInteraction(scopeKey?: string | number) {
  const controller = useContext(Controller);
  if (!controller) throw new Error('useInteraction requires an InteractionProvider');
  // A retained screen can end its interaction lifetime before React unmounts it.
  const scope = useMemo(() => controller.createScope(), [controller, scopeKey]);
  useLayoutEffect(() => {
    scope.activate();
    return scope.dispose;
  }, [scope]);
  return scope.interaction;
}
