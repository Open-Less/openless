#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$ROOT/.vendor" "$ROOT/.build-tools"
checkout() {
  local url="$1" dir="$2" commit="$3"
  if [ ! -d "$dir/.git" ]; then git init "$dir"; git -C "$dir" remote add origin "$url"; fi
  if ! git -C "$dir" cat-file -e "$commit^{commit}" 2>/dev/null; then git -C "$dir" fetch --depth 1 origin "$commit"; fi
  test "$(git -C "$dir" rev-parse HEAD 2>/dev/null || true)" = "$commit" || git -C "$dir" checkout --detach "$commit"
}
checkout https://github.com/yonaskolb/XcodeGen.git "$ROOT/.build-tools/XcodeGen" 21ac9944b0ab546a07422dbed86f33dd2ebd76f8
checkout https://github.com/rime/librime.git "$ROOT/.vendor/librime" 1c23358157934bd6e6d6981f0c0164f05393b497
git -C "$ROOT/.vendor/librime" submodule update --init --recursive --depth 1
checkout https://github.com/KeyboardKit/KeyboardKit.git "$ROOT/.vendor/KeyboardKit" a7e33ac2e6f47e16288ce09ec8d765cd329f918c
grep -q '^MIT License' "$ROOT/.vendor/KeyboardKit/LICENSE"
cp "$ROOT/.vendor/KeyboardKit/LICENSE" "$ROOT/Resources/Licenses/KeyboardKit-LICENSE.txt"
checkout https://github.com/rime/rime-pinyin-simp.git "$ROOT/.vendor/rime-pinyin-simp" 0c6861ef7420ee780270ca6d993d18d4101049d0
BOOST="$ROOT/.vendor/boost_1_85_0.tar.bz2"
[ -f "$BOOST" ] || curl -LfsS --retry 2 https://archives.boost.io/release/1.85.0/source/boost_1_85_0.tar.bz2 -o "$BOOST"
echo "7009fe1faa1697476bdc7027703a2badb84e849b7b0baad5086b087b971f8617  $BOOST" | shasum -a 256 -c -
[ -d "$ROOT/.vendor/boost_1_85_0/boost" ] || tar -xjf "$BOOST" -C "$ROOT/.vendor" boost_1_85_0/boost boost_1_85_0/LICENSE_1_0.txt
swift build --package-path "$ROOT/.build-tools/XcodeGen" -c release
rustup toolchain install 1.99.0 --profile minimal
rustup target add --toolchain 1.99.0 aarch64-apple-ios aarch64-apple-ios-sim
python3 "$ROOT/scripts/prepare-core.py"
echo "Dependencies ready. Run scripts/build-rime.sh, then scripts/build-core.sh."
