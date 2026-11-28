// Guard nested native Button clicks against enclosing card activation.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
const source = (path) =>
  readFileSync(new URL('../../harmony/entry/src/main/ets/' + path, import.meta.url), 'utf8');
const kit = source('components/Kit.ets');
const keys = { KEYCODE_ENTER: 1, KEYCODE_NUMPAD_ENTER: 2, KEYCODE_SPACE: 3, KEYCODE_A: 4 };
const helper = kit.slice(
  kit.indexOf('export function stopButtonKeyBubbling'),
  kit.indexOf('/** Section heading'),
);
const exports = {};
vm.runInNewContext(
  ts.transpileModule(helper, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText,
  { exports, KeyCode: keys },
);
for (const keyCode of Object.values(keys)) {
  let stops = 0;
  exports.stopButtonKeyBubbling({ keyCode, stopPropagation: () => stops++ });
  assert.equal(stops, keyCode === keys.KEYCODE_A ? 0 : 1);
}
function nativeClick(text, context) {
  const match = text.match(/\.onClick\(\(event: ClickEvent\) => \{([\s\S]*?)\n\s*\}\)/);
  assert.ok(match, 'a nested button must expose an isolated ClickEvent callback');
  const compiled = ts.transpileModule(
    'exports.click = function(event: ClickEvent) {' + match[1] + '\n};',
    {
      compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
    },
  ).outputText;
  const exports = {};
  vm.runInNewContext(compiled, { exports });
  let stopped = false;
  const event = {
    stopPropagation() {
      stopped = true;
    },
  };
  exports.click.call(context, event);
  assert.equal(stopped, false, 'ArkUI ClickEvent has no DOM stopPropagation method');
  assert.doesNotMatch(
    match[1],
    /stopPropagation/,
    'click isolation must use native hit testing, not an unsupported ClickEvent API',
  );
}
for (const name of ['PrimaryButton', 'TonalButton', 'TextButton', 'IconButton']) {
  const begin = kit.indexOf('export struct ' + name);
  const end = kit.indexOf('\n@Component', begin + 1);
  const component = kit.slice(begin, end < 0 ? undefined : end);
  let actions = 0;
  nativeClick(component, { usable: true, onTap: () => actions++ });
  assert.equal(actions, 1, name + ' activates exactly once');
  assert.match(component, /onKeyEvent\(\(event: KeyEvent\) => stopButtonKeyBubbling\(event\)\)/);
}
const card = source('components/library/LibraryPresentation.ets');
const begin = card.indexOf('  capsule()');
const capsule = card.slice(begin, card.indexOf('  build()', begin));
let actions = 0;
nativeClick(capsule, { primaryEnabled: true, onPrimary: () => actions++ });
assert.equal(actions, 1);
nativeClick(capsule, { primaryEnabled: false, onPrimary: () => actions++ });
assert.equal(actions, 1, 'disabled native card actions remain inert');
assert.match(capsule, /Row\(\)\s*\{[\s\S]*Button\(/);
assert.match(
  capsule,
  /\.hitTestBehavior\(HitTestMode\.Block\)/,
  'the disabled capsule wrapper must prevent the underlying card click (Huawei FAQ 243)',
);
assert.match(capsule, /onKeyEvent/);
const navigation = source('components/ProductNavigation.ets');
assert.doesNotMatch(
  navigation,
  /item\([^)]*active:\s*boolean/,
  'ForEach items must not retain an initial computed selection Boolean',
);
assert.match(navigation, /accessibilitySelected\(this\.primaryActive\(id\)\)/);
assert.match(navigation, /backgroundColor\(this\.primaryActive\(id\)/);
assert.match(navigation, /uiMaterial\.isImmersiveMaterialSupported\(\)/);
const shell = source('components/AppShell.ets');
assert.match(shell, /Tabs\(\{ barPosition: BarPosition\.End \}\)/);
assert.match(shell, /\.tabBar\(this\.floatingDock\)/);
assert.match(
  shell,
  /\.barOverlap\(true\)/,
  'native bottom TabBar must overlay content rather than reserve an opaque base',
);
const dockHead =
  navigation
    .slice(0, navigation.indexOf('  build()'))
    .replace(/^import .*;\n/gm, '')
    .replace(/^@Component\n/gm, '')
    .replace(/@(Prop|StorageLink)\('[^']*'\)\s*/g, '')
    .replace(/@Prop\s+/g, '')
    .replace('export struct ProductDock', 'export class ProductDock') + '\n}\n';
const dockExports = {};
const deviceInfo = { sdkApiVersion: 23 };
let supportChecks = 0;
let supported = false;
const uiMaterial = {
  isImmersiveMaterialSupported() {
    supportChecks++;
    return supported;
  },
  ImmersiveStyle: { THIN: 1 },
  ImmersiveMaterial: class {
    constructor(options) {
      this.options = options;
    }
  },
};
vm.runInNewContext(
  ts.transpileModule(dockHead, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText,
  {
    exports: dockExports,
    deviceInfo,
    uiMaterial,
    console,
    Palette: { dockTint: () => '#ffffff' },
    Nav: {
      OVERVIEW: 'overview',
      STYLE: 'style',
      TRANSLATION: 'translation',
      rootOf: (route) => (route === 'credentials' ? 'settings' : route),
    },
  },
);
const dock = new dockExports.ProductDock();
for (const route of ['overview', 'history', 'vocab', 'settings', 'overview']) {
  dock.route = route;
  for (const id of ['overview', 'history', 'vocab', 'settings']) {
    assert.equal(dock.primaryActive(id), id === route, 'selection follows live route: ' + route);
  }
}
dock.route = 'style';
assert.equal(dock.primaryActive('vocab'), false);
dock.route = 'translation';
assert.equal(dock.primaryActive('vocab'), false);
console.log(
  'Harmony click surfaces: native activation, key/hit-test isolation and live dock selection passed',
);

// HarmonyOS 6.1/API 23 is the minimum; API 26 capability queries and modifiers
// must be fully skipped, not invoked with an undefined material.
{
  assert.equal(dock.dockMaterial(), undefined);
  assert.equal(supportChecks, 0, 'API 23 never queries the API 26 module');
  const unavailable = {
    get systemMaterial() {
      throw new Error('API 26 attribute accessed on API 23');
    },
  };
  new dockExports.DockMaterialModifier({ fixture: 'material' }).applyNormalAttribute(unavailable);
  deviceInfo.sdkApiVersion = 26;
  assert.equal(dock.dockMaterial(), undefined, 'unsupported hardware uses the blur fallback');
  new dockExports.DockMaterialModifier(undefined).applyNormalAttribute(unavailable);
  supported = true;
  const material = dock.dockMaterial();
  assert.ok(material);
  const applied = [];
  new dockExports.DockMaterialModifier(material).applyNormalAttribute({
    systemMaterial: (value) => applied.push(value),
  });
  assert.deepEqual(applied, [material]);
  assert.doesNotMatch(
    navigation,
    /\.systemMaterial\(this\.mat\)/,
    'new attributes cannot be unconditionally called by older devices',
  );
  assert.equal(
    (navigation.match(/\.attributeModifier\(this\.materialModifier\)/g) ?? []).length,
    2,
  );
}
console.log(
  'HarmonyOS 6.1/7 material compatibility: API 23 skip, API 26 hardware fallback and guarded application passed',
);
