/** Presentation categories inferred only from actual tool names, never arguments or filenames. */
export type ToolActivityCategory = 'search' | 'read' | 'command' | 'edit' | 'web' | 'other';
export type ToolActivityState = 'active' | 'finished' | 'stopped';

export interface ToolActivityEvent {
  name: string;
  running: boolean;
}

export interface ToolActivityGroup {
  category: ToolActivityCategory;
  state: ToolActivityState;
  tools: ToolActivityEvent[];
  names: { name: string; count: number }[];
}

const CATEGORIES: Record<string, ToolActivityCategory> = {
  grep: 'search',
  glob: 'search',
  search: 'search',
  websearch: 'search',
  searchquery: 'search',
  findfiles: 'search',
  searchfiles: 'search',
  listdirectory: 'search',
  listfiles: 'search',
  ls: 'search',
  read: 'read',
  readfile: 'read',
  readtextfile: 'read',
  readmultiplefiles: 'read',
  cat: 'read',
  bash: 'command',
  shell: 'command',
  terminal: 'command',
  execcommand: 'command',
  runcommand: 'command',
  executeshell: 'command',
  run: 'command',
  edit: 'edit',
  write: 'edit',
  writefile: 'edit',
  applypatch: 'edit',
  multiedit: 'edit',
  strreplace: 'edit',
  strreplaceeditor: 'edit',
  webfetch: 'web',
  fetch: 'web',
  browse: 'web',
  openurl: 'web',
  browsernavigate: 'web',
};

export function toolActivityCategory(name: string): ToolActivityCategory {
  const parts = name.trim().split(/__|[.:/]/);
  const operation = (parts[parts.length - 1] ?? '').replace(/[\s_-]/g, '').toLowerCase();
  return CATEGORIES[operation] ?? 'other';
}

/**
 * Consecutive calls in one category form a display step. "finished" means the
 * activity stream advanced, not that a tool returned success; no result payload
 * exists in this protocol. Terminal errors/cancellation stop the trailing step.
 */
export function groupToolActivities(
  tools: readonly ToolActivityEvent[],
  working: boolean,
  interrupted = false,
): ToolActivityGroup[] {
  const groups: ToolActivityGroup[] = [];
  for (const tool of tools) {
    const category = toolActivityCategory(tool.name);
    let group = groups[groups.length - 1];
    if (!group || group.category !== category) {
      group = { category, state: 'finished', tools: [], names: [] };
      groups.push(group);
    }
    group.tools.push(tool);
    const previous = group.names[group.names.length - 1];
    if (previous?.name === tool.name) previous.count += 1;
    else group.names.push({ name: tool.name, count: 1 });
    if (working && tool.running) group.state = 'active';
  }
  if (!working && interrupted && groups.length > 0) groups[groups.length - 1].state = 'stopped';
  return groups;
}
