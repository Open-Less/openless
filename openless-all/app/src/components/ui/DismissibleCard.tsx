import { forwardRef, useRef, type Ref } from 'react';
import {
  animate,
  motion,
  useDragControls,
  useMotionValue,
  useComposedRefs,
  type HTMLMotionProps,
} from 'framer-motion';

export const DismissibleCard = forwardRef<
  HTMLDivElement,
  HTMLMotionProps<'div'> & {
    dismissible: boolean;
    onDismiss: () => void;
    /** Keep the owner's ref out of AnimatePresence's child props on React 18. */
    elementRef?: Ref<HTMLDivElement>;
  }
>(function DismissibleCard({ dismissible, onDismiss, elementRef, style, ...props }, ref) {
  const composedRef = useComposedRefs(ref, elementRef);
  const controls = useDragControls();
  const x = useMotionValue(0);
  const cancelled = useRef(false);
  return (
    <motion.div
      {...props}
      ref={composedRef}
      style={{ ...style, x, touchAction: 'pan-y' }}
      drag={dismissible ? 'x' : false}
      dragControls={controls}
      dragListener={false}
      dragMomentum={false}
      onPointerDown={(event) => {
        cancelled.current = false;
        if (
          !dismissible ||
          event.button !== 0 ||
          (event.target as Element).closest('button,input,textarea,a,select')
        )
          return;
        controls.start(event);
      }}
      onPointerCancelCapture={() => {
        cancelled.current = true;
      }}
      onDragEnd={(_, info) => {
        if (
          !cancelled.current &&
          (Math.abs(info.offset.x) > 120 ||
            (Math.abs(info.offset.x) > 40 && Math.abs(info.velocity.x) > 650))
        )
          onDismiss();
        else animate(x, 0, { type: 'spring', stiffness: 480, damping: 38 });
      }}
    />
  );
});
