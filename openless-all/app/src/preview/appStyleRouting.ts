import type { AppStyleAssignment, AppStyleRouting } from '../lib/types';

const routing: AppStyleRouting = {
  categories: {},
  apps: [
    {
      app: { id: 'macos:com.microsoft.VSCode', name: 'Visual Studio Code' },
      category: 'coding',
      seen: true,
      stylePackId: null,
    },
    {
      app: { id: 'macos:com.tencent.xinWeChat', name: 'WeChat' },
      category: 'chat',
      seen: true,
      stylePackId: null,
    },
    {
      app: { id: 'macos:com.apple.mail', name: 'Mail' },
      category: 'email',
      seen: true,
      stylePackId: null,
    },
    {
      app: { id: 'macos:com.apple.Notes', name: 'Notes' },
      category: null,
      seen: true,
      stylePackId: null,
    },
  ],
};

function snapshot(): AppStyleRouting {
  return structuredClone(routing);
}

export const previewAppStyleRouting = {
  snapshot,
  set(assignment: AppStyleAssignment): AppStyleRouting {
    if (assignment.target === 'category') {
      if (assignment.stylePackId) {
        routing.categories[assignment.category] = assignment.stylePackId;
      } else {
        delete routing.categories[assignment.category];
      }
    } else {
      const entry = routing.apps.find(({ app }) => app.id === assignment.appId);
      if (!entry) throw new Error('Application has not been observed');
      entry.stylePackId = assignment.stylePackId;
      routing.apps = routing.apps.filter((app) => app.seen || app.stylePackId);
    }
    return snapshot();
  },
  clearHistory(): AppStyleRouting {
    routing.apps = routing.apps
      .filter((app) => app.stylePackId)
      .map((app) => ({ ...app, seen: false }));
    return snapshot();
  },
};
