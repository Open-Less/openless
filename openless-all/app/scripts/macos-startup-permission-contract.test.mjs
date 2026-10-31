import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const appRoot = resolve(new URL('..', import.meta.url).pathname);
const lib = readFileSync(resolve(appRoot, 'src-tauri/src/lib.rs'), 'utf8');
const permissions = readFileSync(resolve(appRoot, 'src-tauri/src/commands/permissions_cmds.rs'), 'utf8');

assert.match(
  lib,
  /permission_requires_visible_start\([\s\S]*?permissions::check_accessibility\(\)[\s\S]*?permissions::check_microphone\(\)[\s\S]*?permission_recovery_required/,
  'macOS startup must inspect both permissions before suppressing a minimized main window',
);
assert.match(
  lib,
  /start_minimized[\s\S]*?!permission_recovery_required/,
  'missing permissions must override silent start',
);
assert.match(
  permissions,
  /request_accessibility_permission[\s\S]*?reset_tcc_service_for_restart\("Accessibility", "guided authorization retry"\)/,
  'guided accessibility retry must reset the current app TCC identity before requesting access',
);
console.log('macOS startup permission visibility and guided TCC retry contract passed');
