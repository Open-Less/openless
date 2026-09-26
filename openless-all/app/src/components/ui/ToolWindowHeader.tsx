import type { ReactNode } from 'react';
import { X } from 'lucide-react';

/** Shared chrome for native utility windows; controls never become drag targets. */
export function ToolWindowHeader({
  icon,
  title,
  description,
  onClose,
  closeLabel,
  closeDisabled = false,
  draggable = true,
}: {
  icon: ReactNode;
  title: string;
  description: ReactNode;
  onClose: () => void;
  closeLabel: string;
  closeDisabled?: boolean;
  draggable?: boolean;
}) {
  const drag = draggable ? { 'data-tauri-drag-region': true } : {};
  return (
    <header className="ol-tool-header" {...drag}>
      <div className="ol-tool-mark" aria-hidden="true" {...drag}>
        {icon}
      </div>
      <div className="ol-tool-heading" {...drag}>
        <h1 {...drag}>{title}</h1>
        <div className="ol-tool-description" {...drag}>
          {description}
        </div>
      </div>
      <button
        type="button"
        className="ol-tool-close"
        onClick={onClose}
        onMouseDown={(event) => {
          event.preventDefault();
          event.stopPropagation();
        }}
        disabled={closeDisabled}
        aria-label={closeLabel}
        title={closeLabel}
      >
        <X size={16} strokeWidth={1.8} />
      </button>
    </header>
  );
}
