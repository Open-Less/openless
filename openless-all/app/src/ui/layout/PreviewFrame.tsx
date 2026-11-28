import type { ReactNode } from 'react';
import './viewport.css';

export function PreviewFrame({
  width,
  height,
  children,
}: {
  width: number;
  height: number;
  children: ReactNode;
}) {
  return (
    <div className="ol-preview-scroll">
      <div className="ol-preview-frame" style={{ width, height }}>
        {children}
      </div>
    </div>
  );
}
