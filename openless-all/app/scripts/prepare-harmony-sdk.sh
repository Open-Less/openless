#!/usr/bin/env bash
#
# Prepare a HarmonyOS SDK on a hosted runner.
#
#   scripts/prepare-harmony-sdk.sh
#
# The DevEco / HarmonyOS SDK that hvigor accepts is not downloadable without a
# Huawei account, so it cannot be fetched from a public URL the way the
# OpenHarmony SDK can. What other projects do (and what this script implements)
# is: keep one verified tarball of the SDK somewhere the runner may read it,
# pin its SHA-256, and lay it out exactly as DevEco does:
#
#   $DEVECO_SDK_HOME/default/openharmony/{ets,js,native,previewer,toolchains}
#   $DEVECO_SDK_HOME/default/hms/{ets,js,native,toolchains}
#
# Environment:
#   HARMONY_SDK_URL     https URL of the SDK tarball (required)
#   HARMONY_SDK_SHA256  expected SHA-256 of that tarball (required)
#   OPENLESS_HARMONY_TOOLCHAIN  install root (default: $HOME/.local/share/harmony/current)
#   HARMONY_HVIGOR_VERSION      @ohos/hvigor version to install (default: 6.26.8)
#   HARMONY_NODE_VERSION        node used by hvigor (default: current node)
#
# The SDK is cached by the caller (actions/cache) keyed on HARMONY_SDK_SHA256, so
# the download happens once per SDK revision.
set -euo pipefail

url=${HARMONY_SDK_URL:-}
sha=${HARMONY_SDK_SHA256:-}
root=${OPENLESS_HARMONY_TOOLCHAIN:-$HOME/.local/share/harmony/current}
hvigor_version=${HARMONY_HVIGOR_VERSION:-6.26.8}

[ -n "$url" ] || { echo "HARMONY_SDK_URL is required" >&2; exit 1; }
[ -n "$sha" ] || { echo "HARMONY_SDK_SHA256 is required" >&2; exit 1; }

if [ -d "$root/sdk/default/openharmony/ets" ]; then
  echo "SDK already present at $root"
else
  archive=$(mktemp)
  echo "downloading $url"
  curl --fail --location --retry 3 --output "$archive" "$url"
  actual=$(sha256sum "$archive" | awk '{print $1}')
  if [ "$actual" != "$sha" ]; then
    echo "SDK archive checksum mismatch: got $actual, expected $sha" >&2
    exit 1
  fi
  staging=$(mktemp -d)
  tar -xf "$archive" -C "$staging"
  # Accept either a flat `default/...` tree or a top-level directory that
  # contains it, so the tarball does not need a fixed shape.
  if [ ! -d "$staging/default/openharmony" ]; then
    inner=$(find "$staging" -maxdepth 3 -type d -name openharmony | head -1)
    [ -n "$inner" ] || { echo "SDK archive does not contain default/openharmony" >&2; exit 1; }
    staging=$(dirname "$(dirname "$inner")")
  fi
  mkdir -p "$root/sdk"
  rm -rf "$root/sdk/default"
  cp -a "$staging/default" "$root/sdk/default"
  rm -rf "$archive" "$staging"
  echo "SDK installed at $root/sdk/default"
fi

[ -d "$root/sdk/default/openharmony/toolchains" ] || {
  echo "SDK is missing default/openharmony/toolchains" >&2
  exit 1
}

# hvigor and its HarmonyOS plugin live on the Huawei npm registry, not npmjs.
if [ ! -d "$root/hvigor/node_modules/@ohos/hvigor" ]; then
  mkdir -p "$root/hvigor"
  ( cd "$root/hvigor" \
    && npm init -y > /dev/null \
    && npm install --no-audit --no-fund \
         --registry=https://repo.harmonyos.com/npm/ \
         "@ohos/hvigor@${hvigor_version}" "@ohos/hvigor-ohos-plugin@${hvigor_version}" )
fi
echo "hvigor ready at $root/hvigor"
