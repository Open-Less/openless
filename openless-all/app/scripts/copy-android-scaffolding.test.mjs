#!/usr/bin/env node
import assert from 'node:assert/strict';
import {
  formatStringResource,
  hasStringResource,
  mergeMissingStringResources,
  SHIZUKU_STRINGS_BY_LOCALE,
} from './copy-android-scaffolding.mjs';

const baseXml = `<?xml version="1.0" encoding="utf-8"?>
<resources>
    <string name="app_name">OpenLess</string>
</resources>`;

const partialXml = `<?xml version="1.0" encoding="utf-8"?>
<resources>
    <string name="openless_shizuku_permission_rationale">exists</string>
</resources>`;

const mergedAll = mergeMissingStringResources(baseXml, SHIZUKU_STRINGS_BY_LOCALE.values);
assert.equal(mergedAll.changed, true);
assert.ok(hasStringResource(mergedAll.content, 'openless_shizuku_permission_rationale'));
assert.ok(hasStringResource(mergedAll.content, 'openless_shizuku_permission_blocked'));
assert.ok(hasStringResource(mergedAll.content, 'openless_shizuku_open_manager'));

const mergedPartial = mergeMissingStringResources(partialXml, SHIZUKU_STRINGS_BY_LOCALE.values);
assert.equal(mergedPartial.changed, true);
assert.ok(hasStringResource(mergedPartial.content, 'openless_shizuku_open_manager'));
assert.ok(!mergedPartial.content.includes('exists</string>\n    <string name="openless_shizuku_permission_rationale">'));

const mergedTwice = mergeMissingStringResources(mergedAll.content, SHIZUKU_STRINGS_BY_LOCALE.values);
assert.equal(mergedTwice.changed, false);

for (const [locale, stringsByName] of Object.entries(SHIZUKU_STRINGS_BY_LOCALE)) {
  const localeMerged = mergeMissingStringResources(baseXml, stringsByName);
  assert.equal(localeMerged.changed, true, `locale ${locale} should merge`);
  for (const name of Object.keys(stringsByName)) {
    assert.ok(hasStringResource(localeMerged.content, name), `${locale} missing ${name}`);
  }
}

assert.match(
  formatStringResource('openless_shizuku_open_manager', 'Open Shizuku'),
  /<string name="openless_shizuku_open_manager">Open Shizuku<\/string>/,
);

console.log('copy-android-scaffolding string merge checks passed');
