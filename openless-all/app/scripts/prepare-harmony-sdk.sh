#!/usr/bin/env bash
#
# Prepare a HarmonyOS SDK on a hosted runner.
#
#   scripts/prepare-harmony-sdk.sh
#
# Why this exists: `hvigor` refuses the public OpenHarmony SDK. The two trees are
# not interchangeable - verified against `@ohos/hos-sdkmanager-common` (the loader
# hvigor 6 uses for `runtimeOS: HarmonyOS`):
#
#   * component metadata is `sdk-pkg.json` with the payload under a `data` key
#     (the OpenHarmony SDK ships `oh-uni-package.json` with the payload inline);
#   * component directories are named `<path>-<os version build>`, e.g.
#     `ets-HarmonyOS-NEXT2` (`HOS_CONFIG.sdkSufNameMapper`), and each component
#     carries `hosSdkVersion`/`stage` that map to an API level through
#     `HOS_CONFIG.osVersionMapper`;
#   * `HosLocalComponentLoader` accepts a component only when its location matches
#     `<sdkRoot>/<generated name>` (or sits in a directory called `default`).
#
# So the SDK has to be produced by DevEco / the HarmonyOS SDK Manager, which is
# account-gated. What other HarmonyOS projects do - and what this script
# implements - is to keep one verified tarball of that tree where CI may read it,
# pin its SHA-256, and restore it from cache on every run.
#
# A public mirror of the official command-line tools is the default source, which
# is exactly what other HarmonyOS projects do (sherpa-onnx's `harmony-os` workflow
# downloads the same archive and caches it by name). Override the URL/checksum when
# a newer tools release is needed; the checksum is what makes the download
# trustworthy either way.
#
# Environment:
#   HARMONY_SDK_URL     https URL of the tools archive (default below)
#   HARMONY_SDK_SHA256  expected SHA-256 (default below)
#   OPENLESS_HARMONY_TOOLCHAIN  install root (default: $HOME/.local/share/harmony/current)
#   HARMONY_HVIGOR_VERSION      @ohos/hvigor version to install (default: 6.26.8)
#   HARMONY_NPM_REGISTRY        Huawei npm registry (default: https://repo.harmonyos.com/npm/)
set -euo pipefail

# commandline-tools-linux-x64-6.0.1.251.zip, 2,108,510,902 bytes.
DEFAULT_URL=https://huggingface.co/csukuangfj/harmonyos-commandline-tools/resolve/main/commandline-tools-linux-x64-6.0.1.251.zip
DEFAULT_SHA256=e971348eabe959b41b1d07fae037b3cc53ab2c0a8306f0895357dd45f98ad421
url=${HARMONY_SDK_URL:-$DEFAULT_URL}
sha=${HARMONY_SDK_SHA256:-$DEFAULT_SHA256}
root=${OPENLESS_HARMONY_TOOLCHAIN:-$HOME/.local/share/harmony/current}
hvigor_version=${HARMONY_HVIGOR_VERSION:-6.26.8}
registry=${HARMONY_NPM_REGISTRY:-https://repo.harmonyos.com/npm/}
sdk_root=$root/sdk

die() {
  echo "error: $*" >&2
  exit 1
}

# Fail with the loader's own rule instead of letting hvigor report
# "Cannot find the corresponding SDK version under the specified SDK path".
validate_sdk_tree() {
  local dir=$1
  [ -d "$dir" ] || die "SDK root $dir does not exist"
  local packages
  packages=$(find "$dir" -maxdepth 3 -name sdk-pkg.json | head -50)
  if [ -z "$packages" ]; then
    die "no sdk-pkg.json under $dir: this looks like an OpenHarmony SDK (oh-uni-package.json), which hvigor rejects for runtimeOS HarmonyOS"
  fi
  if ! xargs -r grep -l '"apiVersion"' <<< "$packages" > /dev/null; then
    die "sdk-pkg.json under $dir carries no apiVersion; the tarball is not a usable HarmonyOS SDK"
  fi
  # Two shapes exist: one sdk-pkg.json in `default/` describing the whole SDK
  # (what the official command-line tools ship), or one per component directory.
  if [ ! -d "$dir/default/openharmony/ets" ] \
    && [ -z "$(find "$dir" -maxdepth 3 -type d -name 'ets*' | head -1)" ]; then
    die "SDK has no ArkTS component under $dir"
  fi
  if [ ! -d "$dir/default/openharmony/toolchains" ] \
    && [ -z "$(find "$dir" -maxdepth 3 -type d -name 'toolchains*' | head -1)" ]; then
    die "SDK has no toolchains component under $dir"
  fi
  local api
  api=$(xargs -r grep -h '"apiVersion"' <<< "$packages" | head -1 | sed 's/[^0-9]//g')
  echo "SDK ready at $dir (API ${api:-unknown})"
}

# A cache restore can be partial (SDK without `bin/hvigorw`, or the other way
# round), so require both halves before skipping the download.
if [ -n "$(find "$sdk_root" -maxdepth 3 -name sdk-pkg.json 2> /dev/null | head -1)" ] \
  && [ -x "$root/bin/hvigorw" ]; then
  echo "HarmonyOS toolchain already present at $root"
else
  [ -n "$url" ] || die "HARMONY_SDK_URL is required to install the SDK"
  [ -n "$sha" ] || die "HARMONY_SDK_SHA256 is required to install the SDK"

  archive=$(mktemp)
  # Keep the original name so the extractor can tell zip from tar by extension.
  record=$archive.${url##*.}
  echo "downloading $url"
  curl --fail --location --retry 3 --output "$record" "$url"
  actual=$(sha256sum "$record" | awk '{print $1}')
  if [ "$actual" != "$sha" ]; then
    rm -f "$record"
    die "SDK archive checksum mismatch: got $actual, expected $sha"
  fi

  staging=$(mktemp -d)
  case "$record" in
    *.zip)
      command -v unzip > /dev/null 2>&1 || die "unzip is required to extract the tools archive"
      unzip -q "$record" -d "$staging"
      ;;
    *)
      tar -xf "$archive" -C "$staging"
      ;;
  esac
  rm -f "$record"

  # Accept three tarball shapes: a bare SDK tree, a whole command-line-tools
  # install (`sdk/` + `bin/` + `hvigor/` + `tool/`), or either wrapped in one
  # top-level directory.
  source_root=$staging
  if [ ! -d "$staging/sdk" ] && [ -z "$(find "$staging" -maxdepth 3 -name sdk-pkg.json | head -1)" ]; then
    inner=$(find "$staging" -maxdepth 3 -type d -name sdk | head -1)
    [ -n "$inner" ] || die "SDK archive contains neither sdk/ nor sdk-pkg.json"
    source_root=$(dirname "$inner")
  fi

  mkdir -p "$root"
  if [ -d "$source_root/sdk" ]; then
    # Whole toolchain: keep hvigorw and the bundled node, they are what
    # `package-harmony-hap.sh` drives.
    for part in sdk bin hvigor ohpm tool lib; do
      [ -d "$source_root/$part" ] || continue
      rm -rf "${root:?}/$part"
      cp -a "$source_root/$part" "$root/$part"
    done
    echo "HarmonyOS command-line tools installed at $root"
  else
    rm -rf "$sdk_root"
    cp -a "$source_root" "$sdk_root"
    echo "SDK installed at $sdk_root"
  fi
  rm -rf "$staging"
fi

validate_sdk_tree "$sdk_root"

# hvigor comes from two places: the HarmonyOS command-line tools ship it
# (`$root/bin/hvigorw`), and a bare SDK tarball needs it from the Huawei npm
# registry. Never install into the toolchain's own `hvigor/` directory: a second
# `@ohos/hvigor` copy there shadows the bundled one, so `hvigorfile.ts` loads a
# different `hvigor` singleton and every build dies with
# "The root node is not yet available for build".
standalone=$root/standalone-hvigor
if [ -x "$root/bin/hvigorw" ]; then
  echo "hvigor ships with the toolchain at $root/bin/hvigorw"
elif [ -f "$standalone/node_modules/@ohos/hvigor/bin/hvigor.js" ]; then
  echo "hvigor already installed at $standalone"
else
  mkdir -p "$standalone"
  (
    cd "$standalone"
    npm init -y > /dev/null
    npm install --no-audit --no-fund --registry="$registry" \
      "@ohos/hvigor@${hvigor_version}" "@ohos/hvigor-ohos-plugin@${hvigor_version}"
  )
  echo "hvigor installed at $standalone"
fi
