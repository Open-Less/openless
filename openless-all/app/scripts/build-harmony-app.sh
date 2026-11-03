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
SIGNING_DIR="${OPENLESS_HARMONY_SIGNING_DIR:-$HOME/.local/share/harmony/signing/local-test}"
export OPENLESS_HARMONY_CLIPBOARD_ACCESS="${OPENLESS_HARMONY_CLIPBOARD_ACCESS:-0}"
case "$OPENLESS_HARMONY_CLIPBOARD_ACCESS" in
  0 | 1) ;;
  *)
    echo 'OPENLESS_HARMONY_CLIPBOARD_ACCESS must be 0 or 1' >&2
    exit 1
    ;;
esac
# Keep the debug HAP on the same versionCode ladder as the release packaging:
# a release build carries package-revision, and installing a lower code over it
# fails with 9568263 "install version downgrade" (the only workaround would be
# an uninstall, which wipes the device's app data).
APP_JSON="$APP_ROOT/harmony/AppScope/app.json5"
VERSION_CODE="${OPENLESS_HARMONY_VERSION_CODE:-$(< "$APP_ROOT/linux-egui/package-revision")}"
VERSION_NAME="${OPENLESS_HARMONY_VERSION:-$(node -p "require('$APP_ROOT/package.json').version" | sed 's/[-+].*//')}"
APP_JSON_BACKUP="$(mktemp)"
cp "$APP_JSON" "$APP_JSON_BACKUP"
restore_app_json() { cp "$APP_JSON_BACKUP" "$APP_JSON"; rm -f "$APP_JSON_BACKUP"; }
trap restore_app_json EXIT
python3 - "$APP_JSON" "$VERSION_CODE" "$VERSION_NAME" << 'PY'
import re, sys
path, code, name = sys.argv[1], sys.argv[2], sys.argv[3]
text = open(path).read()
text = re.sub(r'"versionCode":\s*\d+', '"versionCode": ' + code, text)
text = re.sub(r'"versionName":\s*"[^"]*"', '"versionName": "' + name + '"', text)
open(path, 'w').write(text)
PY

BUILD_PROJECT="$CACHE_ROOT/harmony/openless-project"
export OPENLESS_RIME_PREFIX="$CACHE_ROOT/mobile-rime/harmony/install"
if [ ! -f "$OPENLESS_RIME_PREFIX/lib/librime.a" ] || [ ! -f "$CACHE_ROOT/mobile-rime/data/openless-data-version.txt" ]; then
  python3 "$APP_ROOT/scripts/build-mobile-rime.py" harmony
fi
mkdir -p "$BUILD_PROJECT" "$HVIGOR_USER_HOME" "$CACHE_ROOT/npm"
bash "$APP_ROOT/scripts/build-harmony-backend.sh"
# Build a disposable copy so every Hvigor intermediate stays in the shared cache.
python3 - "$APP_ROOT/harmony" "$BUILD_PROJECT" "$SIGNING_DIR" << 'PY'
from pathlib import Path
import json, os, shutil, sys
source, target, signing = map(Path, sys.argv[1:])
shutil.copytree(source, target, dirs_exist_ok=True, ignore=shutil.ignore_patterns('.hvigor', 'build', 'oh_modules', 'local.properties'))
# The ArkWeb shell is gone: drop any bundle an older build staged, otherwise the
# reusable project directory keeps shipping ~40 MB of dead web assets.
stale_web = target / 'entry/src/main/resources/rawfile/web'
if stale_web.exists():
    shutil.rmtree(stale_web)
# The repository ships the phone/tablet declaration because that is what the local
# debug profile grants. Store channels are signed with the AGC release profile, which
# also covers 2-in-1 - declare it there so the release build can actually run on PC
# with free-form windows. A profile without 2in1 would make the system reject the
# app ("不支持 PC 和 2in1 设备"), so this stays tied to the signed channel.
if os.environ.get('OPENLESS_HARMONY_STORE_SIGNING'):
    manifest = target / 'entry/src/main/module.json5'
    text = manifest.read_text()
    text = text.replace('"deviceTypes": [\n      "phone",\n      "tablet"\n    ],',
                        '"deviceTypes": [\n      "phone",\n      "tablet",\n      "2in1"\n    ],')
    text = text.replace('"supportWindowMode": [\n          "fullscreen",\n          "split"\n        ]',
                        '"supportWindowMode": [\n          "fullscreen",\n          "split",\n          "floating"\n        ]')
    manifest.write_text(text)

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
# AppGallery only accepts an App Pack (`.app`), never a bare `.hap`. hvigor
# cannot write a signingConfig from CI: `storePassword`/`keyPassword` must be the
# IDE-encrypted material (>= 32 chars), so the pack stays unsigned here and
# `scripts/sign-harmony-hap.py` signs it afterwards with `hap-sign-tool`.
"${HVIGOR[@]}" --mode module -p product=default -p module=entry@default -p buildMode=debug assembleHap --no-daemon
printf '\nHAP output: %s/entry/build/default/outputs/default/\n' "$BUILD_PROJECT"

if [ -n "${OPENLESS_HARMONY_STORE_SIGNING:-}" ]; then
  # `--mode project`: `assembleApp` is a project-level task and is invisible in
  # module mode ("Task [ 'assembleApp' ] was not found in the project").
  "${HVIGOR[@]}" --mode project -p product=default -p buildMode=release assembleApp --no-daemon
  printf 'App Pack output: %s/build/outputs/default/\n' "$BUILD_PROJECT"
fi

if [[ -f "$SIGNING_DIR/debug.cer" && -f "$SIGNING_DIR/debug.p7b" ]]; then
  python3 "$APP_ROOT/scripts/sign-harmony-hap.py" "$HARMONY_TOOLCHAIN" "$SIGNING_DIR" "$BUILD_PROJECT/entry/build/default/outputs/default"
fi
