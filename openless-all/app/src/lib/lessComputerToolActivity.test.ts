import { groupToolActivities, toolActivityCategory } from './lessComputerToolActivity';

function equal(actual: unknown, expected: unknown, name: string) {
  if (JSON.stringify(actual) !== JSON.stringify(expected))
    throw new Error(`${name}: ${JSON.stringify(actual)} != ${JSON.stringify(expected)}`);
}

for (const [name, category] of [
  ['Grep', 'search'],
  ['mcp__files__read_file', 'read'],
  ['Bash', 'command'],
  ['apply_patch', 'edit'],
  ['WebFetch', 'web'],
  ['provider.some_unknown_tool', 'other'],
])
  equal(toolActivityCategory(name), category, name);

const calls = [
  { name: 'Read', running: false },
  { name: 'Bash', running: false },
  { name: 'Bash', running: false },
  { name: 'Bash', running: true },
];
const grouped = groupToolActivities(calls, true);
equal(
  grouped.map(({ category, state }) => [category, state]),
  [
    ['read', 'finished'],
    ['command', 'active'],
  ],
  'consecutive phase grouping',
);
equal(grouped[1].names, [{ name: 'Bash', count: 3 }], 'dense actual calls retain their count');
equal(
  grouped.flatMap((group) => group.tools),
  calls,
  'every real tool event remains available',
);
equal(
  groupToolActivities(calls, false).map((group) => group.state),
  ['finished', 'finished'],
  'completion removes all active states',
);
equal(
  groupToolActivities(calls, false, true).map((group) => group.state),
  ['finished', 'stopped'],
  'cancel or error cannot claim final step completed',
);
equal(
  groupToolActivities(
    [
      { name: 'Read', running: false },
      { name: 'Bash', running: false },
      { name: 'Read', running: true },
    ],
    true,
  ).length,
  3,
  'do not merge across different stages',
);
equal(groupToolActivities([], true), [], 'no invented activity before a real tool event');
equal(calls[3].running, true, 'projection does not mutate replay state');
console.log('lessComputerToolActivity.test.ts passed');
