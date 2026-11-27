import type { AppStyleAssignment, AppStyleRouting, InputAppCatalog } from '../lib/types';

const routing: AppStyleRouting = {
  catalog: { schemaVersion: 1, reviewedAt: '', categories: [], clients: [] },
  memberships: {},
  categories: {},
  apps: [],
};

function snapshot(): AppStyleRouting {
  const result = structuredClone(routing);
  for (const entry of result.apps) {
    const bundle = entry.app.id.replace(/^macos:/, '').toLowerCase();
    const matched = result.catalog.clients.find(
      ({ identifiers: ids }) =>
        ids.macosBundleIds.some((id) => id.toLowerCase() === bundle) ||
        (ids.macosBundlePrefixes?.some((prefix) => bundle.startsWith(prefix.toLowerCase())) &&
          ids.installedWebAppNames.some(
            (name) => name.toLowerCase() === entry.app.name.toLowerCase(),
          )),
    );
    const client = matched?.canonicalClientId
      ? (result.catalog.clients.find((item) => item.id === matched.canonicalClientId) ?? matched)
      : matched;
    entry.memberId = client ? `catalog:${client.id}` : entry.app.id;
    entry.category = Object.prototype.hasOwnProperty.call(result.memberships, entry.memberId)
      ? result.memberships[entry.memberId]
      : (client?.category ?? null);
  }
  return result;
}

export const previewAppStyleRouting = {
  snapshot,
  async refreshCatalog(): Promise<AppStyleRouting> {
    const url =
      import.meta.env.VITE_OPENLESS_APP_CATALOG_URL ?? 'https://apic.openless.top/v1/app-catalog';
    const response = await fetch(url);
    if (!response.ok) throw new Error('Application catalog API unavailable');
    routing.catalog = (await response.json()) as InputAppCatalog;
    return snapshot();
  },
  set(assignment: AppStyleAssignment): AppStyleRouting {
    if (assignment.target === 'membership') {
      routing.memberships[assignment.memberId] = assignment.category;
    } else if (assignment.target === 'category') {
      if (assignment.stylePackId) routing.categories[assignment.category] = assignment.stylePackId;
      else delete routing.categories[assignment.category];
    } else {
      const entry = routing.apps.find(({ app }) => app.id === assignment.appId);
      if (!entry) throw new Error('Application has not been observed');
      entry.stylePackId = assignment.stylePackId;
      routing.apps = routing.apps.filter(
        (app) =>
          app.seen ||
          app.stylePackId ||
          Object.prototype.hasOwnProperty.call(routing.memberships, app.memberId),
      );
    }
    return snapshot();
  },
  clearHistory(): AppStyleRouting {
    routing.apps = routing.apps
      .filter(
        (app) =>
          app.stylePackId ||
          Object.prototype.hasOwnProperty.call(routing.memberships, app.memberId),
      )
      .map((app) => ({ ...app, seen: false }));
    return snapshot();
  },
};
