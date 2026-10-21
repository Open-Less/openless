#!/usr/bin/env bash
set -euo pipefail
APP_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CACHE_ROOT="${OPENLESS_CACHE_ROOT:-/data-win/openless-cache}"
HARMONY_TOOLCHAIN="${OPENLESS_HARMONY_TOOLCHAIN:-$HOME/.local/share/harmony/current}"
export DEVECO_SDK_HOME="$HARMONY_TOOLCHAIN/sdk"
export DEVECO_NODE_HOME="$HARMONY_TOOLCHAIN/tool/node"
export NODE_HOME="$DEVECO_NODE_HOME"
export PATH="$HARMONY_TOOLCHAIN/bin:$DEVECO_NODE_HOME/bin:$PATH"
export HVIGOR_USER_HOME="$CACHE_ROOT/harmony/hvigor"
export npm_config_cache="$CACHE_ROOT/npm"
export VITE_OPENLESS_PLATFORM=harmony
SIGNING_DIR="${OPENLESS_HARMONY_SIGNING_DIR:-$HOME/.local/share/harmony/signing/local-test}"
export OPENLESS_HARMONY_CLIPBOARD_ACCESS="${OPENLESS_HARMONY_CLIPBOARD_ACCESS:-0}"
export OPENLESS_HARMONY_STARTUP_PROBES="${OPENLESS_HARMONY_STARTUP_PROBES:-0}"
case "$OPENLESS_HARMONY_CLIPBOARD_ACCESS" in
  0|1) ;;
  *) echo 'OPENLESS_HARMONY_CLIPBOARD_ACCESS must be 0 or 1' >&2; exit 1 ;;
esac
case "$OPENLESS_HARMONY_STARTUP_PROBES" in
  0|1) ;;
  *) echo 'OPENLESS_HARMONY_STARTUP_PROBES must be 0 or 1' >&2; exit 1 ;;
esac
BUILD_PROJECT="$CACHE_ROOT/harmony/openless-project"
WEB_DIST="$CACHE_ROOT/harmony/web-dist"
export OPENLESS_RIME_PREFIX="$CACHE_ROOT/mobile-rime/harmony/install"
if [ ! -f "$OPENLESS_RIME_PREFIX/lib/librime.a" ] || [ ! -f "$CACHE_ROOT/mobile-rime/data/openless-data-version.txt" ]; then
  python3 "$APP_ROOT/scripts/build-mobile-rime.py" harmony
fi
mkdir -p "$BUILD_PROJECT" "$HVIGOR_USER_HOME" "$CACHE_ROOT/npm"
bash "$APP_ROOT/scripts/build-harmony-backend.sh"
cd "$APP_ROOT"
npm run build -- --base=./ --outDir="$WEB_DIST"
# Build a disposable copy so every Hvigor intermediate stays in the shared cache.
python3 - "$APP_ROOT/harmony" "$BUILD_PROJECT" "$WEB_DIST" "$SIGNING_DIR" <<'PY'
from pathlib import Path
import json, os, shutil, sys
source, target, web, signing = map(Path, sys.argv[1:])
shutil.copytree(source, target, dirs_exist_ok=True, ignore=shutil.ignore_patterns('.hvigor', 'build', 'oh_modules', 'local.properties'))
if os.environ['OPENLESS_HARMONY_CLIPBOARD_ACCESS'] == '1':
    manifest = target / 'entry/src/main/module.json5'
    module = json.loads(manifest.read_text())
    module['module']['requestPermissions'].append({
        'name': 'ohos.permission.READ_PASTEBOARD',
        'reason': '$string:clipboard_reason',
        'usedScene': {'abilities': ['EntryAbility', 'OpenLessInputMethod'], 'when': 'inuse'},
    })
    manifest.write_text(json.dumps(module, ensure_ascii=False, indent=2))
    flags = target / 'entry/src/main/resources/base/element/boolean.json'
    flags.write_text(json.dumps({'boolean': [{'name': 'ime_clipboard_auto_read', 'value': True}]}))
shutil.copytree(source.parent / 'mobile-ime', target / 'mobile-ime', dirs_exist_ok=True)
shutil.copytree(target.parents[1] / 'mobile-rime/data', target / 'entry/src/main/resources/rawfile/rime', dirs_exist_ok=True)
resources = target / 'entry/src/main/resources/rawfile/web'
if resources.exists():
    shutil.rmtree(resources)
shutil.copytree(web, resources)
# ArkWeb rawfile has an opaque origin. Classic scripts/styles load locally;
# ES modules and crossorigin-tagged assets require a supported origin scheme.
index = resources / 'index.html'
html = index.read_text().replace('type="module" ', '').replace(' crossorigin', '')
# Keep execution after the root element is parsed, matching module-script timing.
html = html.replace('<script ', '<script defer ')
if os.environ['OPENLESS_HARMONY_STARTUP_PROBES'] == '1':
    html = html.replace('</body>', '''<script>
window.addEventListener('error', function(e) { console.error('[OpenLess Boot] ' + e.message); });
window.addEventListener('unhandledrejection', function(e) { console.error('[OpenLess Boot] ' + String(e.reason)); });
window.addEventListener('load', function() {
  setTimeout(function() {
    if (window.OpenLessHarmonyApi && window.OpenLessHarmonyApi.request) {
      var api = window.OpenLessHarmonyApi;
      ['get_startup_snapshot', 'get_settings', 'list_history', 'list_vocab', 'list_microphone_devices', 'check_microphone_permission', 'check_network', 'list_channels', 'list_provider_descriptors', 'marketplace_auth_status', '__test_style_pack_roundtrip', '__test_rime_roundtrip', '__test_voice_roundtrip'].forEach(function(cmd) {
        var id = api.request(cmd, JSON.stringify({kind:'asr'}));
        var poll = function() {
          var wire = api.takeResult(id);
          if (!wire) { setTimeout(poll, 50); return; }
          var result = JSON.parse(wire);
          console.info('[OpenLess System Probe] ' + JSON.stringify({command:cmd,ok:result.ok,count:Array.isArray(result.value)?result.value.length:undefined,error:result.error}));
        };
        poll();
      });
    }
    var root = document.getElementById('root');
    console.info('[OpenLess Render] ' + JSON.stringify({children: root && root.childElementCount, hasText: !!(root && root.innerText.trim())}));
  }, 2000);
});
</script></body>''')
index.write_text(html)
libs = target / 'entry/src/main/libs/arm64-v8a'
libs.mkdir(parents=True, exist_ok=True)
shutil.copy2(target.parent / 'rust-target/aarch64-unknown-linux-ohos/release/libopenless_harmony_host.so', libs)

PY
cd "$BUILD_PROJECT"
# ohpm keeps its cache under $OHPM_HOME (default ~/.ohpm). The CLI's `--cache`
# flag only exists in newer DevEco builds, so set the home instead of guessing
# which flag this toolchain has.
export OHPM_HOME="${OHPM_HOME:-$CACHE_ROOT/harmony/ohpm}"
ohpm install --all
# `HVIGOR_JS` lets a runner without DevEco drive the standalone hvigor CLI; the
# DevEco install keeps using its own `hvigorw` wrapper.
if [ -n "${HVIGOR_JS:-}" ]; then
  HVIGOR=(node "$HVIGOR_JS")
else
  HVIGOR=(hvigorw)
fi
"${HVIGOR[@]}" --mode module -p product=default -p module=entry@default -p buildMode=debug assembleHap --no-daemon
printf '\nHAP output: %s/entry/build/default/outputs/default/\n' "$BUILD_PROJECT"

if [[ -f "$SIGNING_DIR/debug.cer" && -f "$SIGNING_DIR/debug.p7b" ]]; then
  python3 "$APP_ROOT/scripts/sign-harmony-hap.py" "$HARMONY_TOOLCHAIN" "$SIGNING_DIR" "$BUILD_PROJECT/entry/build/default/outputs/default"
fi
