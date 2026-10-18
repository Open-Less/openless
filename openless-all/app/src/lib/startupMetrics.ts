// Navigation-relative milestones; UI paint is an rAF estimate, not native process launch time.
export type StartupStage = 'frontend' | 'react' | 'backend' | 'interactive' | 'sync';
export function markStartup(stage: StartupStage) {
  const name = `openless:${stage}`;
  if (!performance.getEntriesByName(name).length) performance.mark(name);
}
export function afterPaint(stage: StartupStage) {
  const first = requestAnimationFrame(() => {
    second = requestAnimationFrame(() => markStartup(stage));
  });
  let second = 0;
  return () => {
    cancelAnimationFrame(first);
    cancelAnimationFrame(second);
  };
}
export function startupReport() {
  return {
    clock: 'navigationStart (WebView only)',
    milestones: Object.fromEntries(
      performance
        .getEntriesByType('mark')
        .filter((entry) => entry.name.startsWith('openless:'))
        .map((entry) => [entry.name.slice(9), Math.round(entry.startTime)]),
    ),
    paints: performance.getEntriesByType('paint').map(({ name, startTime }) => ({
      name,
      ms: Math.round(startTime),
    })),
  };
}
