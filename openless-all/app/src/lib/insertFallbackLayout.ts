export interface FallbackCardHeightReport {
  presentationId: number;
  height: number;
}

export function nextFallbackCardHeightReport(
  previous: FallbackCardHeightReport | null,
  presentationId: number,
  measuredHeight: number,
): FallbackCardHeightReport | null {
  if (!Number.isFinite(measuredHeight) || measuredHeight <= 0) return null;
  const height = Math.ceil(measuredHeight);
  if (previous?.presentationId === presentationId && previous.height === height) return null;
  return { presentationId, height };
}
