#!/usr/bin/env bash
#
# Prepare a HarmonyOS 7 (API 26) SDK on a hosted runner.
#
#   ../harmony/scripts/prepare-harmony-sdk.sh
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
# implements - is to keep one verified archive of that tree where CI may read it,
# pin its SHA-256, and rebuild the toolchain from it on every run.
#
# Version contract: Huawei maps 26.0.0 to HarmonyOS 7.0 / API 26 and 6.1.0 to
# API 23 (the project's `compatibleSdkVersion`). The API 26 material APIs
# (`uiMaterial.ImmersiveMaterial`, `systemMaterial`) only exist from API 26, so an
# older tools archive fails inside the ArkTS compiler with "SystemUiMaterial is
# not defined". This script therefore refuses any `sdk-pkg.json` whose
# `apiVersion` is below `HARMONY_MIN_API` instead of letting that surface later.
#
# A community mirror of the official command-line tools is the default source.
# The mirror publishes the 2.28 GB archive as two GitHub release assets because a
# single release asset is limited to 2 GB; the pinned digest covers the
# concatenation in the order listed below. Trust is therefore explicit and
# limited: a fixed whole-archive SHA-256 plus the SDK's own `sdk-pkg.json`
# metadata (`apiVersion`, `stage`) must both verify. This is NOT an official
# Huawei host and provides no signature attestation - point `HARMONY_SDK_URL` /
# `HARMONY_SDK_SHA256` at an account-signed official archive when one is
# available. Override them for any other tools release as well.
#
# Version mapping (official):
#   https://developer.huawei.com/consumer/en/doc/harmonyos-releases/upgrade-adaptation
#   https://developer.huawei.com/consumer/en/doc/doccenter-dev-faq/app-compatibility-influence-factor
#
# Environment:
#   HARMONY_SDK_PARTS   space-separated archive shard URLs (default below)
#   HARMONY_SDK_URL     single archive URL; overrides HARMONY_SDK_PARTS
#   HARMONY_SDK_SHA256  expected SHA-256 of the complete archive (default below)
#   HARMONY_MIN_API     minimum `sdk-pkg.json` apiVersion accepted (default: 26)
#   HARMONY_SDK_REPLACE 1 to replace an existing toolchain below HARMONY_MIN_API
#   OPENLESS_HARMONY_TOOLCHAIN  install root (default: $HOME/.local/share/harmony/current)
#   HARMONY_HVIGOR_VERSION      @ohos/hvigor version to install (default: 6.26.8)
#   HARMONY_NPM_REGISTRY        Huawei npm registry (default: https://repo.harmonyos.com/npm/)
set -euo pipefail

# HarmonyOS 7 command-line tools 26.0.0.821 (Release). Two shards:
# 1,572,864,000 bytes + 700,317,187 bytes. The digest below is the mirror's
# published SHA-256 of the concatenated tar.gz, not of either shard.
DEFAULT_PARTS="https://github.com/ErBWs/ohos-sdk/releases/download/26.0.0.821/ohos-sdk-linux-amd64.tar.gz.aa https://github.com/ErBWs/ohos-sdk/releases/download/26.0.0.821/ohos-sdk-linux-amd64.tar.gz.ab"
DEFAULT_SHA256=0cbdf7ac5c1be1e42694d448ffaee0c0be0ba9a948197e2bc5b61ca5bdc481f2
sha=${HARMONY_SDK_SHA256:-$DEFAULT_SHA256}
min_api=${HARMONY_MIN_API:-26}
replace_existing=${HARMONY_SDK_REPLACE:-0}
if [ -n "${HARMONY_SDK_URL:-}" ]; then
  parts=("$HARMONY_SDK_URL")
else
  read -r -a parts <<< "${HARMONY_SDK_PARTS:-$DEFAULT_PARTS}"
fi
root=${OPENLESS_HARMONY_TOOLCHAIN:-$HOME/.local/share/harmony/current}
hvigor_version=${HARMONY_HVIGOR_VERSION:-6.26.8}
registry=${HARMONY_NPM_REGISTRY:-https://repo.harmonyos.com/npm/}
sdk_root=$root/sdk

die() {
  echo "error: $*" >&2
  exit 1
}

# `[ "$api" -lt "$min_api" ]` must never receive a non-number.
case "$min_api" in
  '' | *[!0-9]*) die "HARMONY_MIN_API must be a positive integer, got '$min_api'" ;;
esac
[ "$min_api" -ge 1 ] || die "HARMONY_MIN_API must be at least 1, got '$min_api'"

# The only authority for the API level is the SDK's own `sdk-pkg.json`; a tools
# build number (26.0.0.821) or a directory name never stands in for it.
sdk_metadata() {
  # POSIX awk so the same parsing runs on Linux and macOS hosts; BSD tools lack
  # GNU-only flags such as `xargs -r`.
  awk -v key="$2" '
    index($0, "\"" key "\"") {
      value = substr($0, index($0, "\"" key "\""))
      sub(/^"[^"]*"[ \t]*:[ \t]*/, "", value)
      sub(/^"/, "", value)
      sub(/".*$/, "", value)
      sub(/,[ \t]*$/, "", value)
      sub(/[ \t]+$/, "", value)
      print value
      exit
    }' "$1"
}

# `sha256sum` is GNU; macOS ships `shasum`.
sha256_of() {
  if command -v sha256sum > /dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

sdk_api_version() {
  local dir=$1 file
  file=$(find "$dir" -maxdepth 3 -name sdk-pkg.json 2> /dev/null | head -1)
  [ -n "$file" ] || return 0
  sdk_metadata "$file" apiVersion
}

# Fail with the loader's own rule instead of letting hvigor report
# "Cannot find the corresponding SDK version under the specified SDK path".
# $2 = 0 validates a staged candidate silently, 1 (default) announces the final
# tree; the candidate is checked before anything replaces the install root.
validate_sdk_tree() {
  local dir=$1
  local announce=${2:-1}
  [ -d "$dir" ] || die "SDK root $dir does not exist"
  local packages
  packages=$(find "$dir" -maxdepth 3 -name sdk-pkg.json | head -50)
  if [ -z "$packages" ]; then
    die "no sdk-pkg.json under $dir: this looks like an OpenHarmony SDK (oh-uni-package.json), which hvigor rejects for runtimeOS HarmonyOS"
  fi
  local has_api=0 pkg
  for pkg in $packages; do
    if grep -q '"apiVersion"' "$pkg"; then
      has_api=1
      break
    fi
  done
  if [ "$has_api" != "1" ]; then
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
  local api stage metadata
  metadata=$(find "$dir" -maxdepth 3 -name sdk-pkg.json | head -1)
  api=$(sdk_metadata "$metadata" apiVersion)
  stage=$(sdk_metadata "$metadata" stage)
  [ -n "$api" ] || die "cannot read apiVersion from $metadata"
  case "$api" in
    '' | *[!0-9]*) die "SDK apiVersion '$api' at $metadata is not a number" ;;
  esac
  if [ "$api" -lt "$min_api" ]; then
    die "SDK API $api at $metadata is below the required API $min_api: HarmonyOS 7 (26.0.0) is needed for the API 26 material APIs; set HARMONY_SDK_URL/HARMONY_SDK_PARTS to an API $min_api archive"
  fi
  if [ "$stage" != "Release" ]; then
    die "SDK stage '${stage:-unknown}' at $metadata is not Release: only a Release SDK may provide the pinned toolchain"
  fi
  if [ "$announce" = "1" ]; then
    echo "SDK ready at $dir (API $api, $stage)"
  fi
}

# A cache restore can be partial (SDK without `bin/hvigorw`, or the other way
# round), so require both halves before skipping the download.
existing_api=$(sdk_api_version "$sdk_root")
if [ -n "$existing_api" ] && [ "$existing_api" -ge "$min_api" ] \
  && [ -x "$root/bin/hvigorw" ]; then
  echo "HarmonyOS toolchain already present at $root (API $existing_api)"
elif [ -n "$existing_api" ] && [ "$existing_api" -lt "$min_api" ] \
  && [ "$replace_existing" != "1" ]; then
  die "existing toolchain at $root has API $existing_api, below the required $min_api; install into a new directory (OPENLESS_HARMONY_TOOLCHAIN) or set HARMONY_SDK_REPLACE=1 to replace it"
else
  [ "${#parts[@]}" -gt 0 ] || die "HARMONY_SDK_URL or HARMONY_SDK_PARTS is required to install the SDK"
  [ -n "${parts[0]:-}" ] || die "HARMONY_SDK_URL or HARMONY_SDK_PARTS is required to install the SDK"
  [ -n "$sha" ] || die "HARMONY_SDK_SHA256 is required to install the SDK"

  work=$(mktemp -d)
  trap 'rm -rf "$work"' EXIT
  shard=0
  for part in "${parts[@]}"; do
    target=$work/part-$(printf '%03d' "$shard")
    echo "downloading $part"
    curl --fail --location --retry 3 --output "$target" "$part"
    shard=$((shard + 1))
  done
  # Keep the original extension so the extractor can tell zip from tar.
  record=$work/archive.tar
  case " ${parts[*]} " in
    *.zip*) record=$work/archive.zip ;;
    *.tar.gz* | *.tgz*) record=$work/archive.tar.gz ;;
  esac
  if [ "$shard" -eq 1 ]; then
    cp "$work/part-000" "$record"
  else
    cat "$work"/part-* > "$record"
  fi
  actual=$(sha256_of "$record")
  if [ "$actual" != "$sha" ]; then
    die "SDK archive checksum mismatch: got $actual, expected $sha"
  fi

  staging=$(mktemp -d "$work/staging.XXXXXX")
  case "$record" in
    *.zip)
      command -v unzip > /dev/null 2>&1 || die "unzip is required to extract the tools archive"
      unzip -q "$record" -d "$staging"
      ;;
    *.tar.gz | *.tgz)
      tar -xzf "$record" -C "$staging"
      ;;
    *)
      tar -xf "$record" -C "$staging"
      ;;
  esac
  rm -f "$record"

  # Accept three tarball shapes: a bare SDK tree, a whole command-line-tools
  # install (`sdk/` + `bin/` + `hvigor/` + `tool/`), or either wrapped in one
  # top-level directory.
  source_root=$staging
  if [ ! -d "$staging/sdk" ]; then
    # Whole toolchains may wrap sdk/ under command-line-tools/. Other tools,
    # notably emulator/, also carry sdk-pkg.json without an apiVersion. Locate
    # the actual sdk/ directory before interpreting loose package metadata as
    # a bare SDK root.
    inner=$(find "$staging" -maxdepth 3 -type d -name sdk | head -1)
    if [ -n "$inner" ]; then
      source_root=$(dirname "$inner")
    elif [ -z "$(find "$staging" -maxdepth 3 -name sdk-pkg.json | head -1)" ]; then
      die "SDK archive contains neither sdk/ nor sdk-pkg.json"
    fi
  fi

  # Validate the candidate while it is still staged: a bad archive, an old API or
  # a non-Release SDK must fail before any existing install root is replaced.
  if [ -d "$source_root/sdk" ]; then
    validate_sdk_tree "$source_root/sdk" 0
  else
    validate_sdk_tree "$source_root" 0
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
