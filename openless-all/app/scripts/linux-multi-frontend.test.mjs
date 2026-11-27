import assert from 'node:assert/strict';
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  copyFileSync,
  chmodSync,
  rmSync,
  readFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import vm from 'node:vm';

const root = new URL('../../../', import.meta.url);
const load = (path) => readFileSync(new URL(path, root), 'utf8');
const temporary = mkdtempSync(join(tmpdir(), 'openless-launcher-'));
try {
  mkdirSync(join(temporary, 'usr/bin'), { recursive: true });
  mkdirSync(join(temporary, 'usr/lib/openless'), { recursive: true });
  mkdirSync(join(temporary, 'usr/share/openless'), { recursive: true });
  const launcher = join(temporary, 'usr/bin/openless');
  copyFileSync(new URL('openless-all/linux/scripts/openless', root), launcher);
  chmodSync(launcher, 0o755);
  writeFileSync(join(temporary, 'usr/share/openless/version'), 'OpenLess test-234\n');
  for (const frontend of ['qt', 'gtk']) {
    const path = join(temporary, `usr/lib/openless/openless-${frontend}`);
    writeFileSync(
      path,
      `#!/usr/bin/env bash\nprintf '${frontend}'; printf ' <%s>' "$@"; printf '\\n'\n`,
    );
    chmodSync(path, 0o755);
  }
  const run = (args = []) => spawnSync(launcher, args, { encoding: 'utf8' });
  assert.equal(run().stdout.trim(), 'qt <>');
  assert.equal(run(['--frontend=gtk', '--minimized']).stdout.trim(), 'gtk <--minimized>');
  assert.equal(run(['--minimized', '--frontend', 'gtk']).stdout.trim(), 'gtk <--minimized>');
  assert.equal(run(['--frontend=qt']).status, 0);
  assert.equal(run(['--frontend=other']).status, 2);
  assert.equal(run(['--frontend']).status, 2);
  assert.equal(run(['--bad']).status, 2);
  assert.equal(run(['--version']).stdout.trim(), 'OpenLess test-234');
  assert.match(run(['--help']).stdout, /Default frontend: qt/);

  // Exercise the shared injected bridge itself, including a read-only browser
  // Clipboard object. Direct native copy must survive a failed optional shim.
  const header = load('openless-all/linux/shared/web_host_bridge.h');
  const source = header.match(/R"OPENLESS\(([\s\S]*?)\)OPENLESS"/)[1];
  const messages = [];
  const window = { addEventListener() {}, __openlessEngine: 'webkitgtk' };
  const clipboard = Object.freeze({ writeText() {} });
  const context = {
    window,
    navigator: { clipboard },
    document: { documentElement: { dataset: {} } },
    alert: (message) => messages.push(message),
    fetch: () => Promise.resolve({}),
    console,
  };
  vm.runInNewContext(source, context);
  await window.__openlessCopyText('history text 中文');
  window.__openlessSetInputRegions([]);
  assert.deepEqual(messages, ['openless-copy:history text 中文', 'openless-input-regions:[]']);
  assert.equal(context.document.documentElement.dataset.olEngine, 'webkitgtk');

  const qt = load('openless-all/linux/qt/src/main.cpp');
  const gtk = load('openless-all/linux/gtk/src/main.cpp');
  for (const host of [qt, gtk]) {
    assert.ok(host.includes('/openless.lock'));
    assert.ok(host.includes('/openless-host.sock'));
    assert.ok(host.includes('kOpenlessWebHostScript'));
  }
  assert.match(qt, /XShapeCombineRectangles/);
  assert.match(gtk, /gdk_surface_set_input_region/);
  console.log('Linux dual-frontend launcher, shared native copy and input-region contracts passed');
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
