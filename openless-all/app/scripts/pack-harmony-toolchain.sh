#!/usr/bin/env bash
#
# Pack a HarmonyOS toolchain for CI.
#
#   scripts/pack-harmony-toolchain.sh [output.tar.zst]
#
# The HarmonyOS SDK that hvigor accepts cannot be downloaded without a Huawei
# account, so CI restores a tarball instead (see scripts/prepare-harmony-sdk.sh).
# This script produces that tarball from a local install of the HarmonyOS
# command-line tools.
#
# It packs the SDK plus `hvigorw`, `hvigor/`, `ohpm/` and the bundled node, and
# skips only the emulator. hvigor refuses to build when any of the expected
# components (ets, js, native, previewer, toolchains) is missing, so those all
# have to travel with the tarball.
#
# The result is printed with its size and SHA-256; those two values are what the
# release workflow needs (`HARMONY_SDK_URL`, `HARMONY_SDK_SHA256`).
set -euo pipefail

toolchain=${OPENLESS_HARMONY_TOOLCHAIN:-$HOME/.local/share/harmony/current}
# `current` is a symlink into a versioned directory; tar would archive the link
# itself, so pack the real target.
[ -d "$toolchain" ] && toolchain=$(readlink -f "$toolchain")
[ -d "$toolchain/sdk" ] || { echo "no toolchain at $toolchain" >&2; exit 1; }
[ -x "$toolchain/bin/hvigorw" ] || { echo "$toolchain/bin/hvigorw is missing" >&2; exit 1; }

output=${1:-$PWD/harmony-toolchain.tar.zst}
mkdir -p "$(dirname "$output")"

# Pack an explicit member list: excluding `previewer` by name would also drop the
# hvigor plugin's own `tasks/hook/previewer` module, which every build imports.
name=$(basename "$toolchain")
members=()
for part in sdk bin hvigor ohpm tool lib LICENSE.txt NOTICE.txt version.txt; do
  [ -e "$toolchain/$part" ] && members+=("$name/$part")
done

echo "packing $toolchain (${#members[@]} top-level entries)"
tar_args=(-cf - -C "$(dirname "$toolchain")"
  --exclude="$name/emulator"
  --exclude='node_modules/.cache')
if command -v zstd > /dev/null 2>&1; then
  tar "${tar_args[@]}" "${members[@]}" | zstd -10 -T0 --quiet -f -o "$output"
else
  echo "zstd not found, falling back to gzip (slower and larger)" >&2
  output=${output%.zst}.gz
  tar "${tar_args[@]}" "${members[@]}" | gzip -6 -c > "$output"
fi

bytes=$(stat -c %s "$output")
printf 'size: %s bytes (%.1f MiB)\n' "$bytes" "$(awk -v b="$bytes" 'BEGIN {print b/1048576}')"
if [ "$bytes" -gt 2147483648 ]; then
  echo "warning: larger than the 2 GiB GitHub Release asset limit; host it on object storage or split the archive" >&2
fi
echo "sha256: $(sha256sum "$output" | awk '{print $1}')"
echo "$output"
