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
import shutil, sys
source, target, web, signing = map(Path, sys.argv[1:])
shutil.copytree(source, target, dirs_exist_ok=True, ignore=shutil.ignore_patterns('.hvigor', 'build', 'oh_modules', 'local.properties'))
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
ohpm install --all --cache "$CACHE_ROOT/harmony/ohpm"
hvigorw --mode module -p product=default -p module=entry@default -p buildMode=debug assembleHap --no-daemon
printf '\nHAP output: %s/entry/build/default/outputs/default/\n' "$BUILD_PROJECT"

if [[ -f "$SIGNING_DIR/debug.cer" && -f "$SIGNING_DIR/debug.p7b" ]]; then
  python3 "$APP_ROOT/scripts/sign-harmony-hap.py" "$HARMONY_TOOLCHAIN" "$SIGNING_DIR" "$BUILD_PROJECT/entry/build/default/outputs/default"
fi
