import { readFile } from 'node:fs/promises';

const panelUrl = new URL(
  '../android/frontend/components/AndroidPermissionsPanel.tsx',
  import.meta.url,
);
const source = await readFile(panelUrl, 'utf8');
const imports = [...source.matchAll(/import\s+([\s\S]*?)\s+from\s+['"]([^'"]+)['"];?/g)];

function moduleFor(name) {
  return imports
    .filter(([, bindings]) => new RegExp(`\\b${name}\\b`).test(bindings))
    .map(([, , moduleName]) => moduleName);
}

for (const name of ['getSettings', 'setSettings']) {
  const modules = moduleFor(name);
  if (modules.length !== 1 || modules[0] !== '../../../src/lib/ipc/settings') {
    throw new Error(
      `${name} must be imported directly from ../../../src/lib/ipc/settings; found ${JSON.stringify(modules)}`,
    );
  }
}

if (/from\s+['"]\.\.\/\.\.\/\.\.\/src\/lib\/ipc['"]/.test(source)) {
  throw new Error('AndroidPermissionsPanel must not import through the IPC barrel');
}

console.log('android-ipc-import-boundary.test.mjs passed');
