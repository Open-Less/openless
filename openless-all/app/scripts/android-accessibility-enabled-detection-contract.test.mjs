import { readFile } from 'node:fs/promises';

const serviceUrl = new URL(
  '../android/kotlin/OpenLessAccessibilityService.kt',
  import.meta.url,
);
const jniUrl = new URL('../src-tauri/src/android/jni.rs', import.meta.url);
const panelUrl = new URL(
  '../android/frontend/components/AndroidPermissionsPanel.tsx',
  import.meta.url,
);
const accessibilityUrl = new URL('../src-tauri/src/android/accessibility.rs', import.meta.url);

const serviceSource = await readFile(serviceUrl, 'utf8');
const jniSource = await readFile(jniUrl, 'utf8');
const panelSource = await readFile(panelUrl, 'utf8');
const accessibilitySource = await readFile(accessibilityUrl, 'utf8');

const allSources = [serviceSource, jniSource, panelSource, accessibilitySource].join('\n');

if (allSources.includes('DBG-21a66f')) {
  throw new Error('DBG-21a66f diagnostic markers must be removed');
}

if (allSources.includes('127.0.0.1:7807')) {
  throw new Error('localhost debug ingest fetch must be removed');
}

if (jniSource.includes('accessibility_settings_debug')) {
  throw new Error('jni.rs must not retain accessibility_settings_debug');
}

if (/services\.contains\(expected\)/.test(serviceSource)) {
  throw new Error(
    'OpenLessAccessibilityService.isEnabled must not use naive services.contains(expected)',
  );
}

if (!/OpenLessAccessibilityComponentIds\.enabledListContains/.test(serviceSource)) {
  throw new Error(
    'OpenLessAccessibilityService.isEnabled must delegate to OpenLessAccessibilityComponentIds.enabledListContains',
  );
}

if (!/@Keep[\s\S]*fun isEnabled\(context: Context\)/.test(serviceSource)) {
  throw new Error('OpenLessAccessibilityService.isEnabled must be annotated with @Keep for JNI/R8');
}

if (!/@Keep[\s\S]*fun pingAccessibilityProcess\(context: Context\)/.test(serviceSource)) {
  throw new Error(
    'OpenLessAccessibilityService.pingAccessibilityProcess must be annotated with @Keep for JNI/R8',
  );
}

if (/services\.contains\(&component_id\)/.test(jniSource)) {
  throw new Error('jni.accessibility_enabled must not use naive services.contains(&component_id)');
}

if (!/enabled_services_contain/.test(jniSource)) {
  throw new Error('jni.accessibility_enabled must call enabled_services_contain');
}

if (
  !/status\.enabled\s*&&\s*status\.operational\s*===\s*false/.test(panelSource)
) {
  throw new Error(
    'AndroidAccessibilityStatusPill must retain enabled=true && operational=false branch',
  );
}

console.log('android-accessibility-enabled-detection-contract.test.mjs passed');
