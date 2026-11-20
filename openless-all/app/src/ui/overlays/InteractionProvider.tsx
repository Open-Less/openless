import {
  createContext,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useState,
  type ComponentType,
  type ReactNode,
} from 'react';
import { createInteractionController, type InteractionController } from './interactionController';
import { createToast } from '../feedback/toast';
import { ConfirmDialogHost } from './ConfirmDialogHost';
import { bindNotificationSink } from '../../lib/notifications';
import { NotificationCenter } from '../feedback/NotificationCenter';
import './interaction.css';

const Controller = createContext<ReturnType<typeof createInteractionController> | null>(null);

export function InteractionProvider({
  children,
  notificationAside,
  notificationHost: NotificationHost = NotificationCenter,
}: {
  children: ReactNode;
  notificationAside?: ReactNode;
  notificationHost?: ComponentType<{ controller: InteractionController; aside?: ReactNode }>;
}) {
  const [controller] = useState(createInteractionController);
  useLayoutEffect(() => {
    const scope = controller.createScope();
    const unbind = bindNotificationSink({
      notify: scope.interaction.notify,
      dismissByKey: controller.dismissByKey,
    });
    return () => {
      unbind();
      scope.dispose();
    };
  }, [controller]);
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
      <NotificationHost controller={controller} aside={notificationAside} />
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

/** Non-blocking feedback over the current window's interaction controller. */
export function useToast(scopeKey?: string | number) {
  const { notify } = useInteraction(scopeKey);
  return useMemo(() => createToast(notify), [notify]);
}
